# intersect-ingress-proxy

Effectively serves as a way for two message brokers to "share" messages across HTTPS. These applications connect to each other through server-sent events, allowing for messages to be pushed without waiting for a response.

This repository consists of two applications, neither of which can function without the other:

- `broker-2-http` - subscribe to message brokers and emit the messages on a SSE endpoint
- `http-2-broker` - subscribe to the aforementioned SSE endpoint and publish them to a broker.

Currently only supports AMQP 0-9-1 as the broker protocol but can potentially support others in the future

## Why Rust?

- great at handling tons of concurrent requests, necessary for something like this
- strong static analysis
- Axum has great community support and examples, and is async (this is the webserver maintained by the [tokio runtime team](https://tokio.rs/), the most supported async runtime for Rust).

The best way to install Rust is through [Rustup](https://www.rust-lang.org/tools/install).

You can format with `cargo fmt` and lint with `cargo clippy`

## Application startup

You will need two message brokers and two instances of this application running for it to have any purpose.

1) Spin up backing services: `docker compose up -d` (note that this spins up two brokers, add 1 to all normal port numbers for second broker)
2) In terminal 1, run broker-2-http: `APP_CONFIG_FILE=broker-2-http/conf.yaml cargo run --bin broker-2-http`
3) In terminal 2, run http-2-broker (will not work until broker-2-http is initialized): `APP_CONFIG_FILE=http-2-broker/conf.yaml cargo run --bin http-2-broker`

## Application Configuration (primarily for DevOps)

Common configuration structures can be found in `shared-deps/src/configuration.rs` . The `get_configuration()` function is what will be called to initialize the configuration logic.

Specific configuration structs are in `broker-2-http/src/configuration.rs` and `http-2-broker/src/configuration.rs` .

## Setup

### Using the RabbitMQ web management UIs

These instructions assume you are using the docker compose configuration and the default `conf.yaml` configurations for each.

1) Make sure that you have both applications started (do NOT start more than 1 of each)
2) Login to localhost:15673, username `intersect_username`, password `intersect_password`, click on `exchanges`, make sure the `intersect-messages` exchange exists (this one gets created by `http-2-broker` on startup).
3) Go to the `Queues and streams` tab, expand `Add a new queue` section, set `Name` field as whatever you want, other settings are fine, click `Add queue`
4) Click on the queue you just created, expand `Bindings` section, see `Add binding to queue` section, set `From exchange` to equal `intersect-messages`, set `Routing key` to be `organization.facility.system.subsystem.service.userspace` , click `Bind queue`

(TODO - should probably configure HTTP2BROKER and INTERSECT-SDK Publishers to create a temporary durable queue so that there's always a queue available to consume the message. Then we can skip steps 2/3/4. Note that you can probably skip these steps anyways in production IF you always deploy your Services before executing any Clients, but if Clients execute before Services you may end up losing a message)

5) Login to localhost:15672, username `intersect_username`, password `intersect_password`, click on `exchanges`, check the `intersect-messages` exchange (this one gets created by `broker-2-http` on startup), expand the `Publish message` section, set routing key to `organization.facility.system.subsystem.service.userspace` (this parallels how the SDK constructs these routing keys)
6) Set payload to below:

```
{"messageId":"39d9c119-3b0a-474e-ae3d-f3eb5f8d3a86","operationId":"say_hello_to_name","contentType":"application/json","payload":"\"hello_client\"","headers":{"destination":"tmp-4b19600a-527d-4a0b-9bf7-f500d9656350.tmp-.tmp-.-.tmp-","source":"organization.facility.system.subsystem.service","sdk_version":"0.6.2","created_at":"2024-06-28T15:14:39.117515Z","data_handler":0,"has_error":false}}
```

(This closely replicates what an INTERSECT message looks like, though the only thing you really need to check is that your conf.yaml's `topic_prefix` value starts with the value for `headers.source`)

7) Click "publish_message"
8) On localhost:15673 `Queues and streams` section, blow up `Get messages`, set Ack mode to `Automatic ack`, click on `Get messages`, you should see your payload from step 6.

Congratulations, you have successfully simulated a publisher and a subscriber being able to talk to each other across 2 separate message brokers. 

### Testing with INTERSECT-SDK directly

NOTE - the AMQP logic of the SDK currently needs to be updated to handle this

Now it's advisable to [run some INTERSECT-SDK examples](https://github.com/INTERSECT-SDK/python-sdk/tree/main/examples). One client and one service should do. You should make sure that you're using AMQP configuration settings instead of MQTT configuration settings. Both of the counter examples should work - you should make sure that client and server are talking to DIFFERENT brokers, or the exercise is pointless.

## Possible future features

- support publishing to and subscribing from multiple brokers at once
- support protocols other than AMQP
- support listening to multiple URLs at once (fairly easy feature to add)

## AMQP setup

- one exchange for all messages (see `intersect_messaging.rs` to get name)
- routing keys will match SDK naming schematics (SOS hierarchy, "." as separator, end with ".{userspace|lifecycle|events}")
- queue name will also match SDK naming schematics (might need to be careful here, there's a 128 character limit here). This can really be whatever but we need to construct the queue name as something idempotent, we don't really want to have a billion durable queues floating around. 
