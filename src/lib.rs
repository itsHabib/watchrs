//! `watchrs` is a crate that aids in monitoring and setting up alerts for AWS Batch Jobs.

#![warn(missing_docs)]

use log::{error, info};
use rusoto_core::region::Region;
use rusoto_events::{
    CloudWatchEvents, CloudWatchEventsClient, PutRuleRequest, PutTargetsRequest, Target,
};
use rusoto_sns::{
    CreateTopicInput, DeleteTopicInput, Sns, SnsClient, SubscribeInput, UnsubscribeInput,
};

use chrono::prelude::*;
use serde::Serialize;

// {
//"detail": {
//"jobName": "event-test",
//"jobId": "4c7599ae-0a82-49aa-ba5a-4727fcce14a8",
//"jobQueue": "arn:aws:batch:us-east-1:aws_account_id:job-queue/HighPriority",
//"status": "RUNNABLE",
//"attempts": [],
//"createdAt": 1508781340401,
//"retryStrategy": {
//"attempts": 1
//},

/// An enum whos varients describe the point of faliure during AWS calls.
/// For now, the value captured by the enum only contains a short description of the error.
pub enum WatchError {
    /// Indicates a failure while creating or deleting an SNS Topic
    SNSTopic(String),
    /// Indicates a failure while subscribing or unsibscribing from a topic
    SNSSubscription(String),
    /// Indicates a failure when creating or deleting an event rule
    EventRule(String),
    /// Indicates a failure when creating or deleting a event target
    EventTarget(String),
}

/// Represents a subset of the details field an AWS Batch Event returns.
/// The struct fields are used to filter a rule expression where Batch jobs are the target.
/// The full structure of a batch job event can be found in the [AWS Documentation](https://docs.aws.amazon.com/batch/latest/userguide/batch_cwe_events.html).
#[derive(Hash, Eq, PartialEq, Serialize)]
struct BatchRuleDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "jobName")]
    job_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "jobQueue")]
    queue_arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "jobId")]
    job_id: Option<String>,
}

impl Default for BatchRuleDetails {
    fn default() -> Self {
        BatchRuleDetails {
            status: None,
            job_name: None,
            queue_arn: None,
            job_id: None,
        }
    }
}

/// Creates a topic when `topic_arn is None and suscribes to it using the email provided.
/// The method will skip the topic creation step whenever the `topic_arn` field `is_some()`,.
/// `subscribe` will return back a tuple of the form (topic_arn, subscribtion_arn).
pub fn subscribe(email: String, topic_arn: Option<String>) -> Result<(String, String), WatchError> {
    // only create topic if needed
    let arn = if topic_arn.is_none() {
        create_topic()?
    } else {
        topic_arn.expect("missing topic_arn")
    };

    // susbscribe to email and return (topic arn, subscribtion_arn)
    subscribe_email(arn.clone(), email).map(|subscription_arn| (arn, subscription_arn))
}

/// Unsubscribes an email from a topic using the subscription_arn. The topic will
/// also be deleted when the `deleted_topic` is set. If the deleted flag is set the
/// topic arn must be `Some`. Keep in mind that deleting a topic will also delete
/// all of its subscriptions.
// TODO: Consider making this take less params
pub fn unsubscribe(
    subscription_arn: String,
    delete_topic: bool,
    topic_arn: Option<String>,
) -> Result<(), WatchError> {
    let sns_client = SnsClient::new(Region::default());

    // Error case, we cant delete a topic we dont have an arn for
    if delete_topic && topic_arn.is_none() {
        Err(WatchError::SNSTopic("topic arn should be `Some` when `deleted_flag` is true".to_owned()))

    }
    // just delete the topic if the flag is specified. This is okay to do since
    // deleting a topic will also remove all subscriptions.
    else if delete_topic && topic_arn.is_some() {
        let topic_arn = topic_arn.expect("missing topic_arn");
        sns_client
            .delete_topic(DeleteTopicInput {
                topic_arn: topic_arn.clone(),
            })
            .sync()
            .map_err(|err| {
                error!("error deleting topic {}, err: {}", topic_arn, err);
                WatchError::SNSSubscription(err.to_string())
            })
    } else {
        sns_client
            .unsubscribe(UnsubscribeInput {
                subscription_arn: subscription_arn.clone(),
            })
            .sync()
            .map_err(|err| {
                error!(
                    "error unsubscribing from {}, err: {}",
                    subscription_arn, err
                );
                WatchError::SNSSubscription(err.to_string())
            })
    }
}
/// Creates a Cloudwatch Event Rule that watches for AWS Batch job state changes.
///
/// You can create a more restrictive filter expression by using the status, queue_arn, or
/// job_name fields. The rule created will contain these fields in a details struct. An example of
/// a `details` field looks like below. You can get more info on creating rule expressions for
/// batch jobs [here](https://docs.aws.amazon.com/batch/latest/userguide/batch_cwe_events.html).
///```
/// "details": {
///     "jobName": "event-test",
///     "jobId": "4c7599ae-0a82-49aa-ba5a-4727fcce14a8",
///     "jobQueue": "arn:aws:batch:us-east-1:aws_account_id:job-queue/HighPriority",
/// }
///```
///
pub fn create_job_watcher_rule(
    rule_name: String,
    enable: bool,
    rule_description: Option<String>,
    status: Option<String>,
    queue_arn: Option<String>,
    job_name: Option<String>,
    job_id: Option<String>,
) -> Result<String, WatchError> {
    let events_client = CloudWatchEventsClient::new(Region::default());
    let enable_str = if enable { "ENABLED" } else { "DISABLED" };

    let rule_details = BatchRuleDetails {
        status,
        queue_arn,
        job_name,
        job_id,
    };

    let details_json = serde_json::to_string(&rule_details);

    if details_json.is_err() {
        return Err(WatchError::EventRule(
            "failed to serialize batch rule details".to_owned(),
        ));
    }

    let mut event_pattern = r#"{{
        "detail-type": [
            "Batch Job State Change"
        ],
        "source": [
            "aws.batch"
        ]"#
    .to_owned();

    // only add the details str if its not empty. AWS does not allow empty fields.
    if BatchRuleDetails::default() != rule_details {
        event_pattern = format!(
            r#"{{
            {},pass empty string nothing rust
            "details": {}
        }}
        "#,
            event_pattern,
            details_json.expect("err getting details json")
        );
    }

    match events_client
        .put_rule(PutRuleRequest {
            name: rule_name.clone(),
            description: rule_description,
            state: Some(enable_str.to_owned()),
            event_pattern: Some(event_pattern),
            ..PutRuleRequest::default()
        })
        .sync()
    {
        Ok(_) => {
            info!("Succesfully put rule: {}", rule_name.clone());
            Ok(rule_name)
        }
        Err(err) => {
            error!("error putting rule: {}", err);
            Err(WatchError::EventRule(err.to_string()))
        }
    }
}

/// Creates a Cloudwatch Event Target pointed to the SNS topic with the provided arn.
/// The method will also attach the rule `rule_name` to the event target.
pub fn create_sns_target(rule_name: String, topic_arn: String) -> Result<(), WatchError> {
    let events_client = CloudWatchEventsClient::new(Region::default());

    let now = Utc::now();
    let (year, month, day, hour) = (now.year(), now.month(), now.day(), now.hour());
    let target_id = format!("watchrs_sns_target_{}_{}_{}_{}", year, month, day, hour);

    let sns_target = Target {
        id: target_id,
        arn: topic_arn.clone(),
        ..Target::default()
    };

    events_client
        .put_targets(PutTargetsRequest {
            rule: rule_name.clone(),
            targets: vec![sns_target],
        })
        .sync()
        .map_err(|err| {
            error!("error putting targets: {}", err);
            WatchError::EventTarget(err.to_string())
        })
        .and_then(|resp| {
            let failed_entries = resp.failed_entries.unwrap_or_default();
            if !failed_entries.is_empty() {
                error!("failed to put targets: {:?}", failed_entries);
                Err(WatchError::EventTarget(format!(
                    "failed entries: {:?}",
                    failed_entries
                )))
            } else {
                info!(
                    "Succesfully put target with rule: {}, on {}",
                    rule_name, topic_arn
                );
                Ok(())
            }
        })
}

/// creates a topic
fn create_topic() -> Result<String, WatchError> {
    let sns_client = SnsClient::new(Region::default());

    let now = Utc::now();
    let (year, month, day, hour) = (now.year(), now.month(), now.day(), now.hour());

    // discard everything after @
    // add watchrs + date
    // further limit to 256 characters for the topic name limit
    let topic_name = &format!("watchrs_{}_{}_{}_{}", year, month, day, hour).to_owned();
    sns_client
        .create_topic(CreateTopicInput {
            attributes: None,
            name: topic_name.to_owned(),
        })
        .sync()
        .map_err(|err| {
            error!("error creating topic: {}", err);
            WatchError::SNSTopic(err.to_string())
        })
        .and_then(|resp| {
            if let Some(topic_arn) = resp.topic_arn {
                info!("Succesfully created topic {}", topic_arn.clone());
                Ok(topic_arn)
            } else {
                Err(WatchError::SNSTopic("error creating topic".to_owned()))
            }
        })
}

/// Subscribes the given email to a topic
fn subscribe_email(topic_arn: String, email: String) -> Result<String, WatchError> {
    let sns_client = SnsClient::new(Region::default());
    let sub_input = SubscribeInput {
        protocol: "email-json".to_owned(),
        endpoint: Some(email.clone()),
        topic_arn: topic_arn.clone(),
        ..SubscribeInput::default()
    };
    sns_client
        .subscribe(sub_input)
        .sync()
        .map_err(|err| {
            error!("error creating topic: {}", err);
            WatchError::SNSSubscription(err.to_string())
        })
        .and_then(|resp| {
            if let Some(subscription_arn) = resp.subscription_arn {
                Ok(subscription_arn)
            } else {
                Err(WatchError::EventRule(
                    "err retreiving subscription arn".to_owned(),
                ))
            }
        })
}
