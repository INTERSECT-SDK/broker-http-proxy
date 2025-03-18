# broker-http-proxy

Effectively serves as a way for two message brokers to "share" messages across HTTPS. These applications connect to each other through server-sent events, allowing for messages to be pushed without waiting for a response.

Specific INTERSECT messaging logic has its own module (see `shared-deps/src/intersect_messaging.rs`), so these applications could easily be forked to support another ecosystem with its own messaging protocol.

This repository consists of two applications, neither of which can function without the other:

- `proxy-http-server` - subscribe to message brokers and emit the messages on a SSE endpoint
- `proxy-http-client` - subscribe to the aforementioned SSE endpoint and publish them to a broker.

Currently only supports AMQP 0-9-1 as the broker protocol but can potentially support others in the future

## Why Rust?

- great at handling tons of concurrent requests, necessary for something like this
- strong static analysis
- Axum has great community support and examples, and is async (this is the webserver maintained by the [tokio runtime team](https://tokio.rs/), the most supported async runtime for Rust).

The best way to install Rust is through [Rustup](https://www.rust-lang.org/tools/install).

You can format with `cargo fmt` and lint with `cargo clippy` . There's also a pre-commit hook you can use ([installation instructions](https://pre-commit.com/#installation))

## Application startup

You will need two message brokers and two instances of this application running for it to have any purpose.

1) Spin up backing services: `docker compose up -d` (note that this spins up two brokers, add 1 to all normal port numbers for second broker)
2) In terminal 1, run proxy-http-server: `APP_CONFIG_FILE=proxy-http-server/conf.yaml cargo run --bin proxy-http-server`
3) In terminal 2, run proxy-http-client (will not work until proxy-http-server is initialized): `APP_CONFIG_FILE=proxy-http-client/conf.yaml cargo run --bin proxy-http-client`

## Application Configuration (primarily for DevOps)

Common configuration structures can be found in `shared-deps/src/configuration.rs` . The `get_configuration()` function is what will be called to initialize the configuration logic.

Specific configuration structs are in `proxy-http-server/src/configuration.rs` and `proxy-http-client/src/configuration.rs` .

## Setup

### Using the RabbitMQ web management UIs

These instructions assume you are using the docker compose configuration and the default `conf.yaml` configurations for each.

1) Make sure that you run `docker compose up -d` from the root directory, to start both brokers and their associated management UIs.
2) Make sure that you have both applications started (do NOT start more than 1 of each). Each application should be connected to a separate broker.
3) To login to the broker that the server instance uses, go to localhost:15672, username `intersect_username`, password `intersect_password`
4) To login to the broker that the client instance uses, go to localhost:15673, username `intersect_username`, password `intersect_password`
5) On each application, click on the `Exchanges` tab, and click on the `intersect-messages` exchange.
6) Make sure that the `Publish message` dropdown is expanded, select the large text area which is labeled with `Payload:`

For the application on `localhost:15672`, set the payload to below (no newlines):

```
{"messageId":"39d9c119-3b0a-474e-ae3d-f3eb5f8d3a86","operationId":"say_hello_to_name","contentType":"application/json","payload":"\"hello_client\"","headers":{"destination":"tmp-4b19600a-527d-4a0b-9bf7-f500d9656350.tmp-.tmp-.-.tmp-","source":"organization.facility.system.subsystem.service","sdk_version":"0.6.2","created_at":"2024-06-28T15:14:39.117515Z","data_handler":0,"has_error":false}}
```

For the application on `localhost:15673`, set the payload to below (no newlines):

```
{"messageId":"39d9c119-3b0a-474e-ae3d-f3eb5f8d3a86","operationId":"say_hello_to_name","contentType":"application/json","payload":"\"hello_client\"","headers":{"destination":"tmp-4b19600a-527d-4a0b-9bf7-f500d9656350.tmp-.tmp-.-.tmp-","source":"organization.facility.system2.subsystem.service","sdk_version":"0.6.2","created_at":"2024-06-28T15:14:39.117515Z","data_handler":0,"has_error":false}}
```

7) Click "publish_message". At this point the message should show up in the logs for both `proxy-http-client` and `proxy-http-server` .
8) Check the logs of the application NOT connected to the broker you just published to. (With the default configuration: check `proxy-http-client` if you published to localhost:15672, check `proxy-http-server` if you published to localhost:15673.) You should see logs which look like this:

```
  2025-03-18T00:09:19.973776Z DEBUG intersect_ingress_proxy_common::protocols::amqp::subscribe: got raw message data from broker: {"messageId":"39d9c119-3b0a-474e-ae3d-f3eb5f8d3a86","operationId":"say_hello_to_name","contentType":"application/json","payload":"\"hello_client\"","headers":{"destination":"tmp-4b19600a-527d-4a0b-9bf7-f500d9656350.tmp-.tmp-.-.tmp-","source":"organization.facility.system.subsystem.service","sdk_version":"0.6.2","created_at":"2024-06-28T15:14:39.117515Z","data_handler":0,"has_error":false}}
    at shared-deps/src/protocols/amqp/subscribe.rs:141

  2025-03-18T00:09:19.973811Z  DEBUG intersect_ingress_proxy_common::protocols::amqp::subscribe: message source is not from this system, will not broadcast it
    at shared-deps/src/protocols/amqp/subscribe.rs:147
```

To explain this log, the basic workflow is this:

1) Application 1 receives a message from Broker 1
2) The message source is from the same system, so it will be published to an HTTP endpoint
3) The message is sent over HTTP: if the http-server gets a message, it broadcasts the message out to all connected clients on the SSE endpoint; if the http-client gets a message, it POSTS the message to the HTTP server. Assuming the send is successful, the message is acked and permanently removed from Broker 1
4) Application 2 gets the message over HTTP and publishes it to Broker 2
5) Application 2 receives this message from Broker 2.
6) The message source is NOT from the same system, so it is not published through HTTP but will be acked to remove it from the broker.

Congratulations, you have successfully simulated a publisher and a subscriber being able to talk to each other across 2 separate message brokers.

### Testing with INTERSECT-SDK directly

Now it's advisable to [run some INTERSECT-SDK examples](https://github.com/INTERSECT-SDK/python-sdk/tree/main/examples). One client and one service should do. You should make sure that you're using AMQP configuration settings instead of MQTT configuration settings. Both of the counter examples should work - you should make sure that client and server are talking to DIFFERENT brokers, or the exercise is pointless.

## Possible future features

- support publishing to and subscribing from multiple brokers at once
- support protocols other than AMQP
- support listening to multiple URLs at once (fairly easy feature to add)
- find way to persist messages which get pulled off of a broker but are not sent over HTTP

## AMQP setup

- one exchange for all messages for each application (see `shared-deps/src/protocols/amqp/mod.rs` to get name)
- routing keys will match SDK naming schematics (SOS hierarchy, "." as separator, end with ".{userspace|lifecycle|events}"). The routing key will roughly correspond to the `destination` field in an INTERSECT message, but the `destination` field only exists on userspace messages (event/lifecycle messages do not have a specific destination in mind).
- The queue name is hardcoded to match the name of the application.
