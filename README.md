# watchrs

[![Crates.io](https://img.shields.io/crates/v/watchrs.svg)](https://crates.io/crates/watchrs)
[![Documentation](https://docs.rs/watchrs/badge.svg)](https://docs.rs/watchrs/)
[![Build Status](https://travis-ci.org/itsHabib/watchrs.svg?branch=master)](https://travis-ci.org/itsHabib/watchrs)

`watchrs` is a crate that aids in monitoring and setting up alerts for AWS Batch Jobs.

## Examples

### Setting Up Alerts For Batch Job State Changes
```rust, no_run
use watchrs::Watcher;

// First create and subscribe to a topic
let watcher = Watcher::default();
watcher
    .subscribe("michaelhabib1868@gmail.com".to_owned(), None)
    .and_then(|(topic_arn, _)| {
        watcher
            .create_job_watcher_rule(
                "my_batch_job_rule".to_owned(),
                // enable?
                true,
                Some("watch failed jobs".to_owned()),
                Some(vec!["FAILED".to_owned(), "RUNNABLE".to_owned()]),
                Some(vec!["JOB_QUEUE_ARN".to_owned()]),
                Some(vec!["JOB_DEFINITION_NAME".to_owned()])
            )
            .map(|rule_name| (topic_arn, rule_name))
    })
      .and_then(|(topic_arn, rule_name)| {
           // create target
           watcher.create_sns_target(rule_name, topic_arn)
    })
    .expect("failed to create alerting system");
