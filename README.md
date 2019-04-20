# watchrs

[![Crates.io](https://img.shields.io/crates/v/watchrs.svg)](https://crates.io/crates/watchrs)
[![Documentation](https://docs.rs/watchrs/badge.svg)](https://docs.rs/watchrs/)
[![Build Status](https://travis-ci.org/itsHabib/watchrs.svg?branch=master)](https://travis-ci.org/itsHabib/watchrs)

`watchrs` is a crate that aids in monitoring and setting up alerts for AWS Batch Jobs.

## Examples

### Setting Up Alerts For Batch Job State Changes
```rust, no_run
## use watchrs::Watcher;

let mut watcher = Watcher::default();
// First create and subscribe to a topic
watcher.subscribe("email@example.com".to_owned(), None)
    .and_then(|(topic_arn, _)| {
        watcher.create_job_watcher_rule(
            "my_batch_job_rule".to_owned(),
             true,
             Some("watch failed jobs".to_owned()),
             Some("FAILED".to_owned()),
             Some("queuearn1234".to_owned()),
             None,
       ).map(|rule_name| {
           (topic_arn, rule_name)
       })
    })
    .and_then(|(topic_arn, rule_name)| {
        // create target
        watcher.create_sns_target(rule_name, topic_arn)
});
