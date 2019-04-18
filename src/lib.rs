use log::{error, info};
use rusoto_core::region::Region;
use rusoto_events::{
    CloudWatchEvents, CloudWatchEventsClient,
    PutTargetsRequest, PutRuleRequest,
    Target,
};
use rusoto_sns::{
    CreateTopicInput,
    Sns, SnsClient, SubscribeInput,
};

use chrono::prelude::*;

pub enum WatchError {
    SNSTopic(String),
    SNSSubscription(String),
    EventRule(String),
    EventTarget(String),
}

pub fn subscribe(email: String, topic_arn: Option<String>) -> Result<String, WatchError> {
    let sns_client = SnsClient::new(Region::default());

    let at_index = if let Some(at_index) = email.find('@') {
        at_index
    } else {
        email.len()
    };
    let now = Utc::now();
    let (year, month, day, hour) = (now.year(), now.month(), now.day(), now.hour());

    // discard everything after @
    // add watchrs + date
    // further limit to 256 characters for the topic name limit
    let topic_name = &format!(
        "watchrs_{}_{}_{}_{}_{}",
        &email[..at_index],
        year,
        month,
        day,
        hour
    )
    .to_owned();

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
        .and_then(|topic_resp| {
            if let Some(topic_arn) = topic_resp.topic_arn {
                info!("Succesfully created topic {}", topic_arn.clone());
                let sub_input = SubscribeInput {
                    protocol: "email-json".to_owned(),
                    endpoint: Some(email.clone()),
                    topic_arn: topic_arn.clone(),
                    ..SubscribeInput::default()
                };
                sns_client
                    .subscribe(sub_input)
                    .sync()
                    .map(|_| topic_arn)
                    .map_err(|err| {
                        error!("error creating topic: {}", err);
                        WatchError::SNSSubscription(err.to_string())
                    })
            } else {
                error!("error creating topic: topic arn not returned");
                Err(WatchError::SNSTopic("topic arn not returned".to_owned()))
            }
        })
}

pub fn unsubscribe() {}

pub fn create_job_watcher_rule(
    rule_name: String,
    status: String,
    enable: bool,
    rule_description: Option<String>,
    queue_arn: Option<String>,
) -> Result<String, WatchError> {
    let events_client = CloudWatchEventsClient::new(Region::default());
    let enable_str = if enable { "ENABLED" } else { "DISABLED" };

    // TODO: make a rule struct that can be deserialized into this
    //
    // create a list of detail params
    // filter for Some() values
    // unwrap each and reduce into a valid json string
    let details: Vec<String> = vec![
        ("\"job_queue\"".to_owned(), queue_arn.clone()),
        (
            "\"status\"".to_owned(),
            Some(format!("[\"{}\"]", status.clone())),
        ),
    ]
    .into_iter()
    .filter(|detail_opt| detail_opt.1.is_some())
    // safe because of filter above
    .map(|detail_opt| format!("{}: {}", detail_opt.0, detail_opt.1.unwrap()))
    .collect();

    // reduce into details json string
    let details_str = details.join(",");

    let event_pattern = format!(
        r#"{{
        "detail-type": [
            "Batch Job State Change"
        ],
        "source": [
            "aws.batch"
        ],
        "details": {{
            {}
        }}
        }}"#,
        details_str
    );

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
            Err(WatchError::EventRule(format!(
                "error putting rule: {}",
                err
            )))
        }
    }
}

// Batch rule -> SNS
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

