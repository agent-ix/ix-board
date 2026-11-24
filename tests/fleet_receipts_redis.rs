// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Agent-IX

//! The live order-receipt reader against a throwaway Redis, written through relay-core's order
//! path. Needs `docker`; the container is removed at the end of the test.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use ix_board::fleet::live::read_relay_on;
use ix_board::fleet::{ExpiredOrder, OrderReceipts};
use ix_trace_rs::trace;
use relay_core::receipts::{DeliveryEvidence, OrderSpec};
use relay_core::{
    AgentName, Claim, HerdrPlaceIds, Message, Now, Place, Popper, Registration, Relay, Target,
};
use std::num::NonZeroUsize;
use std::process::Command;
use std::time::Duration;

struct Container(String);

impl Container {
    fn start() -> (Self, u16) {
        let out = Command::new("docker")
            .args(["run", "-d", "--rm", "-p", "127.0.0.1::6379", "redis:7"])
            .output()
            .expect("docker");
        assert!(out.status.success(), "docker run failed");
        let id = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        let me = Self(id);
        let port = Command::new("docker")
            .args(["port", &me.0, "6379/tcp"])
            .output()
            .unwrap();
        let text = String::from_utf8(port.stdout).unwrap();
        let port = text
            .lines()
            .next()
            .and_then(|l| l.rsplit(':').next())
            .and_then(|p| p.trim().parse().ok())
            .expect("mapped port");
        (me, port)
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        let _ = Command::new("docker").args(["rm", "-f", &self.0]).output();
    }
}

fn connect(port: u16) -> Relay {
    let url = format!("redis://127.0.0.1:{port}");
    for _ in 0..50 {
        if let Ok(mut relay) = Relay::connect(&url, Duration::from_secs(2))
            && relay.lane_lists().is_ok()
        {
            return relay.with_prefix("t:".to_owned());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("redis did not come up");
}

fn lane(relay: &mut Relay, session: &str, tab: &str) -> (AgentName, String) {
    let machine = "terra".parse().unwrap();
    let session_id: relay_core::SessionId = session.parse().unwrap();
    let name = AgentName::for_session(&machine, &session_id);
    let reg = Registration {
        name: name.clone(),
        machine: machine.clone(),
        kind: "claude".into(),
        session_id: Some(session_id.clone()),
        cwd: None,
        herdr_pane: Some("w1:p1".parse().unwrap()),
        place: Some(Place {
            workspace: "SWM".into(),
            tab: tab.into(),
            herdr_ids: Some(HerdrPlaceIds {
                workspace: "w1".parse().unwrap(),
                tab: "w1:t1".parse().unwrap(),
                pane: "w1:p1".parse().unwrap(),
                session_id,
                verified_at_ms: 1,
            }),
        }),
        last_seen_ms: 1,
    };
    relay.register(&reg).unwrap();
    let addr = reg.verified_place().unwrap().address(&machine).to_string();
    (name, addr)
}

fn order_to(relay: &mut Relay, name: &AgentName) {
    let msg = Message {
        from: "sender".parse().unwrap(),
        to: Target::Agent(name.clone()),
        text: "order".into(),
        sent_at_ms: 42,
        forwarded: false,
    };
    relay
        .send_as(&msg, Some(&OrderSpec::new(10_000, None).unwrap()))
        .unwrap();
}

fn expired(relay: &mut Relay) -> ix_board::fleet::ReceiptRead {
    let (snapshot, receipts) = read_relay_on(relay, Duration::from_secs(2));
    snapshot.unwrap();
    receipts.expired_orders().unwrap()
}

#[trace("FR-034-AC-12")]
#[test]
fn ack_expired_orders_are_read_from_a_real_relay_with_and_without_delivery() {
    let (_container, port) = Container::start();
    let mut relay = connect(port);
    let (delivered, delivered_addr) = lane(&mut relay, "s-one", "one");
    let (undelivered, undelivered_addr) = lane(&mut relay, "s-two", "two");
    order_to(&mut relay, &delivered);
    order_to(&mut relay, &undelivered);

    let batch = relay
        .pop_inbox_budget(
            &delivered,
            NonZeroUsize::new(1).unwrap(),
            1_000_000,
            Claim::new(Popper::Watcher).with_lease(Duration::from_secs(600)),
        )
        .unwrap()
        .batch
        .unwrap();
    relay
        .observe_delivery(
            &delivered,
            batch,
            Popper::Watcher,
            DeliveryEvidence::HerdrPrompt,
            Now::Server,
        )
        .unwrap();

    assert!(
        expired(&mut relay).orders.is_empty(),
        "no order has expired before the relay records it"
    );
    std::thread::sleep(Duration::from_secs(11));
    relay.expire_orders(Now::Server).unwrap();

    let read = expired(&mut relay);
    assert!(read.gaps.is_empty() && read.unresolved.is_empty());
    let mut orders = read.orders;
    orders.sort_by(|a, b| a.addr.cmp(&b.addr));
    let mut want = vec![
        ExpiredOrder {
            addr: delivered_addr,
            delivered: true,
        },
        ExpiredOrder {
            addr: undelivered_addr,
            delivered: false,
        },
    ];
    want.sort_by(|a, b| a.addr.cmp(&b.addr));
    assert_eq!(orders, want);
}
