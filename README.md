# watchrs

[![Crates.io](https://img.shields.io/crates/v/watchrs.svg)](https://crates.io/crates/watchrs)
[![Documentation](https://docs.rs/watchrs/badge.svg)](https://docs.rs/watchrs/)
[![Build Status](https://travis-ci.org/itsHabib/watchrs.svg?branch=master)](https://travis-ci.org/itsHabib/watchrs)

`watchrs` is a crate that aids in monitoring and setting up alerts for AWS Batch Jobs.

## Examples

### Setting Up Alerts For Batch Job State Changes

```rust
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
```

## Requirements

- AWS Account
- AWS CLI configured

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
