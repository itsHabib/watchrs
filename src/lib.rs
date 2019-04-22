//! `watchrs` is a crate that aids in monitoring and setting up alerts for AWS Batch Jobs.
//!
//! # Examples
//!
//! ## Setting Up Alerts For Batch Job State Changes
//! ```rust, no_run
//! use watchrs::Watcher;
//!
//! // First create and subscribe to a topic
//! let watcher = Watcher::default();
//! watcher
//!     .subscribe("michaelhabib1868@gmail.com".to_owned(), None)
//!     .and_then(|(topic_arn, _)| {
//!         watcher
//!             .create_job_watcher_rule(
//!                 "my_batch_job_rule".to_owned(),
//!                 // enable?
//!                 true,
//!                 Some("watch failed jobs".to_owned()),
//!                 Some(vec!["FAILED".to_owned(), "RUNNABLE".to_owned()]),
//!                 Some(vec!["JOB_QUEUE_ARN".to_owned()]),
//!                 Some(vec!["JOB_DEFINITION_NAME".to_owned()])
//!             )
//!             .map(|rule_name| (topic_arn, rule_name))
//!     })
//!       .and_then(|(topic_arn, rule_name)| {
//!            // create target
//!            watcher.create_sns_target(rule_name, topic_arn)
//!     })
//!     .expect("failed to create alerting system");
#![deny(missing_docs)]

use log::{error, info};
use rusoto_core::region::Region;
use rusoto_events::{
    CloudWatchEvents, CloudWatchEventsClient, PutRuleRequest, PutTargetsRequest, Target,
};
use rusoto_sns::{
    CreateTopicInput, DeleteTopicInput, Sns, SnsClient, SubscribeInput, UnsubscribeInput,
};
use std::collections::HashMap;

use chrono::prelude::*;
use serde::Serialize;

/// An enum whos varients describe the point of faliure during AWS calls.
/// For now, the value captured by the enum only contains a short description of the error.
#[derive(Hash, Eq, PartialEq, Debug)]
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
#[derive(Hash, Eq, PartialEq, Debug, Serialize)]
struct BatchRuleDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "status")]
    statuses: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "jobName")]
    job_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "jobQueue")]
    job_queues: Option<Vec<String>>,
}

impl Default for BatchRuleDetails {
    fn default() -> Self {
        BatchRuleDetails {
            statuses: None,
            job_names: None,
            job_queues: None,
        }
    }
}

/// Used to create and operate on AWS resources related to monitoring Batch jobs.
///
/// At the moment the `Watcher` struct only takes in a AWS region to indicate
/// where to operate in.
#[must_use]
pub struct Watcher {
    region: Region,
}

impl Default for Watcher {
    fn default() -> Self {
        Watcher {
            region: Region::default(),
        }
    }
}

impl Watcher {
    /// Creates a topic when `topic_arn is None and suscribes to it using the email provided.
    /// The method will skip the topic creation step whenever the `topic_arn` field `is_some()`,.
    /// `subscribe` will return back a tuple of the form (topic_arn, subscribtion_arn).
    ///
    /// ```rust,no_run
    /// # use watchrs::Watcher;
    /// let mut watcher = Watcher::default();
    /// let email = "emailtosubscribe@example.com".to_owned();
    /// let (topic_arn, subscription_arn) = watcher.subscribe(email, None).unwrap();
    /// ```
    pub fn subscribe(
        &self,
        email: String,
        topic_arn: Option<String>,
    ) -> Result<(String, String), WatchError> {
        // only create topic if needed
        let arn = if topic_arn.is_none() {
            self.create_topic()?
        } else {
            topic_arn.expect("missing topic_arn")
        };

        // susbscribe to email and return (topic arn, subscribtion_arn)
        self.subscribe_email(arn.clone(), email)
            .map(|subscription_arn| (arn, subscription_arn))
    }

    /// Unsubscribes an email from a topic using the subscription_arn. The topic will
    /// also be deleted when the `deleted_topic` is set. If the deleted flag is set the
    /// topic arn must be `Some`. Keep in mind that deleting a topic will also delete
    /// all of its subscriptions.
    /// ```rust, no_run
    /// # use watchrs::Watcher;
    /// let mut watcher = Watcher::default();
    /// watcher.unsubscribe("validsubscriptionarn".to_owned(), false, None).unwrap();
    /// ```
    // TODO: Consider making this take less params
    pub fn unsubscribe(
        &self,
        subscription_arn: String,
        delete_topic: bool,
        topic_arn: Option<String>,
    ) -> Result<(), WatchError> {
        let sns_client = SnsClient::new(Region::default());

        // Error case, we cant delete a topic we dont have an arn for
        if delete_topic && topic_arn.is_none() {
            Err(WatchError::SNSTopic(
                "topic arn should be `Some` when `deleted_flag` is true".to_owned(),
            ))
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
    /// You can create a more restrictive filter expression by using the status, job_queues, or
    /// job_name fields. The rule created will contain these fields in a details struct. An example of
    /// a `details` field looks like below. You can get more info on creating rule expressions for
    /// batch jobs [here](https://docs.aws.amazon.com/batch/latest/userguide/batch_cwe_events.html).
    ///
    ///```json
    /// "detail": {
    ///     "jobName": "event-test",
    ///     "jobId": "4c7599ae-0a82-49aa-ba5a-4727fcce14a8",
    ///     "jobQueue": "arn:aws:batch:us-east-1:aws_account_id:job-queue/HighPriority",
    /// }
    ///```
    ///
    /// ```rust, no_run
    /// # use watchrs::Watcher;
    /// let mut watcher = Watcher::default();
    /// watcher.create_job_watcher_rule(
    ///     "my_batch_job_rule".to_owned(),
    ///     true,
    ///     Some("watch failed jobs".to_owned()),
    ///     Some(vec!["FAILED".to_owned()]),
    ///     None,
    ///     None).unwrap();
    /// ```
    pub fn create_job_watcher_rule(
        &self,
        rule_name: String,
        enable: bool,
        rule_description: Option<String>,
        statuses: Option<Vec<String>>,
        job_queues: Option<Vec<String>>,
        job_names: Option<Vec<String>>,
    ) -> Result<String, WatchError> {
        let events_client = CloudWatchEventsClient::new(Region::default());
        let enable_str = if enable { "ENABLED" } else { "DISABLED" };

        let rule_details = BatchRuleDetails {
            statuses,
            job_queues,
            job_names,
        };

        let details_json = serde_json::to_string(&rule_details);

        if details_json.is_err() {
            return Err(WatchError::EventRule(
                "failed to serialize batch rule details".to_owned(),
            ));
        }

        let mut event_pattern = r#"
        "detail-type": [
            "Batch Job State Change"
        ],
        "source": [
            "aws.batch"
        ]
        "#
        .to_owned();

        // only add the details str if its not empty. AWS does not allow empty fields.
        if BatchRuleDetails::default() != rule_details {
            event_pattern = format!(
                r#"{{
                    {},
                    "detail": {}
                    }}
            "#,
                event_pattern,
                details_json.expect("err with details json")
            );
        } else {
            event_pattern = format!("{{{}}}", event_pattern);
        }

        match events_client
            .put_rule(PutRuleRequest {
                name: rule_name.clone(),
                description: rule_description,
                state: Some(enable_str.to_owned()),
                event_pattern: Some(event_pattern),
                role_arn: None,
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
    ///
    /// ```rust, no_run
    /// # use watchrs::Watcher;
    /// let mut watcher = Watcher::default();
    /// let subscription_arn = "validsubscriptionarn".to_owned();
    /// watcher.create_sns_target(
    ///     "my_batch_job_rule".to_owned(),
    ///     "watchrs_topic_arn".to_owned()
    /// ).unwrap();
    /// ```
    pub fn create_sns_target(
        &self,
        rule_name: String,
        topic_arn: String,
    ) -> Result<(), WatchError> {
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

    /// Sets the AWS region a `Watcher` instance should operate in. The default region
    /// is us-east-1. More information on supported services and AWS regions
    /// can be found [here](https://docs.aws.amazon.com/general/latest/gr/rande.html).
    pub fn set_region(&mut self, region: Region) {
        self.region = region
    }

    /// creates a topic
    fn create_topic(&self) -> Result<String, WatchError> {
        let sns_client = SnsClient::new(Region::default());

        let now = Utc::now();
        let (year, month, day, hour) = (now.year(), now.month(), now.day(), now.hour());

        // discard everything after @
        // add watchrs + date
        // further limit to 256 characters for the topic name limit
        let topic_name = &format!("watchrs_{}_{}_{}_{}", year, month, day, hour).to_owned();
        let mut attributes = HashMap::new();
        let sns_access_policy = format!(
            r#"
        {{
            "Id": "AWSSNSCWEIntegration",
            "Statement": [
                {{
                    "Action": [
                        "SNS:GetTopicAttributes",
                        "SNS:SetTopicAttributes",
                        "SNS:AddPermission",
                        "SNS:RemovePermission",
                        "SNS:DeleteTopic",
                        "SNS:Subscribe",
                        "SNS:ListSubscriptionsByTopic",
                        "SNS:Publish",
                        "SNS:Receive"
                    ],
                    "Effect": "Allow",
                    "Principal": {{
                        "AWS": "*"
                    }},
                    "Resource": "arn:aws:sns:{0}:*:{1}",
                    "Sid": "AWSSNSAccess"
                }},
                {{
                    "Sid": "PublishEventsToSNS",
                    "Effect": "Allow",
                    "Principal": {{
                        "Service": "events.amazonaws.com"
                     }},
                    "Action": "sns:Publish",
                    "Resource": "arn:aws:sns:{0}:*:{1}"
                }}
            ],
            "Version": "2008-10-17"
        }}"#,
            self.region.name(),
            topic_name.clone()
        );

        // TODO: Fix this when the bug in rusoto is fixed. or fix rusoto yourself!
        attributes.insert(sns_access_policy.to_owned(), "Policy".to_owned());

        sns_client
            .create_topic(CreateTopicInput {
                attributes: Some(attributes),
                name: topic_name.to_owned(),
            })
            .sync()
            .map_err(|err| {
                error!("error creating topic: {:?}", err);
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
    fn subscribe_email(&self, topic_arn: String, email: String) -> Result<String, WatchError> {
        let sns_client = SnsClient::new(Region::default());
        let sub_input = SubscribeInput {
            protocol: "email".to_owned(),
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
}
