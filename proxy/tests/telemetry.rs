#[macro_use]
extern crate log;

mod support;
use self::support::*;

#[test]
fn inbound_sends_telemetry() {
    let _ = env_logger::init();

    info!("running test server");
    let srv = server::new().route("/hey", "hello").run();

    let mut ctrl = controller::new();
    let reports = ctrl.reports();
    let proxy = proxy::new()
        .controller(ctrl.run())
        .inbound(srv)
        .metrics_flush_interval(Duration::from_millis(500))
        .run();
    let client = client::new(proxy.inbound, "test.conduit.local");

    info!("client.get(/hey)");
    assert_eq!(client.get("/hey"), "hello");

    info!("awaiting report");
    let report = reports.wait().next().unwrap().unwrap();
    // proxy inbound
    assert_eq!(report.proxy, 0);
    // process
    assert_eq!(report.process.as_ref().unwrap().node, "");
    assert_eq!(report.process.as_ref().unwrap().scheduled_instance, "");
    assert_eq!(report.process.as_ref().unwrap().scheduled_namespace, "");
    // requests
    assert_eq!(report.requests.len(), 1);
    let req = &report.requests[0];
    assert_eq!(req.ctx.as_ref().unwrap().authority, "test.conduit.local");
    assert_eq!(req.ctx.as_ref().unwrap().path, "/hey");
    //assert_eq!(req.ctx.as_ref().unwrap().method, GET);
    assert_eq!(req.count, 1);
    assert_eq!(req.responses.len(), 1);
    // responses
    let res = &req.responses[0];
    assert_eq!(res.ctx.as_ref().unwrap().http_status_code, 200);
    // response latencies should always have a length equal to the number
    // of latency buckets into which the controller will aggregate latencies.
    assert_eq!(res.response_latencies.len(), 25);
    assert_eq!(res.ends.len(), 1);
    // ends
    let ends = &res.ends[0];
    assert_eq!(ends.streams.len(), 1);
    // streams
    let stream = &ends.streams[0];
    assert_eq!(stream.bytes_sent, 5);
    assert_eq!(stream.frames_sent, 1);
}

#[test]
fn inbound_aggregates_telemetry_over_several_requests() {
    let _ = env_logger::init();

    info!("running test server");
    let srv = server::new()
        .route("/hey", "hello")
        .route("/hi", "good morning")
        .run();

    let mut ctrl = controller::new();
    let reports = ctrl.reports();
    let proxy = proxy::new()
        .controller(ctrl.run())
        .inbound(srv)
        .metrics_flush_interval(Duration::from_millis(500))
        .run();
    let client = client::new(proxy.inbound, "test.conduit.local");

    info!("client.get(/hey)");
    assert_eq!(client.get("/hey"), "hello");

    info!("client.get(/hi)");
    assert_eq!(client.get("/hi"), "good morning");
    assert_eq!(client.get("/hi"), "good morning");

    info!("awaiting report");
    let report = reports.wait().next().unwrap().unwrap();
    // proxy inbound
    assert_eq!(report.proxy, 0);
    // process
    assert_eq!(report.process.as_ref().unwrap().node, "");
    assert_eq!(report.process.as_ref().unwrap().scheduled_instance, "");
    assert_eq!(report.process.as_ref().unwrap().scheduled_namespace, "");

    // requests -----------------------
    assert_eq!(report.requests.len(), 2);

    // -- first request -----------------
    let req = &report.requests[0];
    assert_eq!(req.ctx.as_ref().unwrap().authority, "test.conduit.local");
    assert_eq!(req.ctx.as_ref().unwrap().path, "/hey");
    assert_eq!(req.count, 1);
    assert_eq!(req.responses.len(), 1);
    // ---- response --------------------
    let res = &req.responses[0];
    assert_eq!(res.ctx.as_ref().unwrap().http_status_code, 200);
    // response latencies should always have a length equal to the number
    // of latency buckets the controller will aggregate latencies with.
    assert_eq!(res.response_latencies.len(), 25);
    assert_eq!(res.ends.len(), 1);

    // ------ ends ----------------------
    let ends = &res.ends[0];
    assert_eq!(ends.streams.len(), 1);
    // -------- streams -----------------
    let stream = &ends.streams[0];
    assert_eq!(stream.bytes_sent, 5);
    assert_eq!(stream.frames_sent, 1);

    // -- second request ----------------
    let req = &report.requests[1];
    assert_eq!(req.ctx.as_ref().unwrap().authority, "test.conduit.local");
    assert_eq!(req.ctx.as_ref().unwrap().path, "/hi");
    // repeated twice
    assert_eq!(req.count, 2);
    assert_eq!(req.responses.len(), 1);
    // ---- response  -------------------
    let res = &req.responses[0];
    assert_eq!(res.ctx.as_ref().unwrap().http_status_code, 200);
    // response latencies should always have a length equal to the number
    // of latency buckets the controller will aggregate latencies with.
    assert_eq!(res.response_latencies.len(), 25);
    assert_eq!(res.ends.len(), 1);

    // ------ ends ----------------------
    let ends = &res.ends[0];
    assert_eq!(ends.streams.len(), 2);

    // -------- streams -----------------
    let stream = &ends.streams[0];
    assert_eq!(stream.bytes_sent, 12);
    assert_eq!(stream.frames_sent, 1);

}

#[test]
fn records_latency_statistics() {
    let _ = env_logger::init();

    info!("running test server");
    let mut srv = server::new()
        .route("/hey", "hello")
        .route("/hi", "good morning");
    &mut srv["/hey"].extra_latency(Duration::from_millis(500));
    &mut srv["/hi"].extra_latency(Duration::from_millis(40));
    let srv = srv.run();

    let mut ctrl = controller::new();
    let reports = ctrl.reports();
    let proxy = proxy::new()
        .controller(ctrl.run())
        .inbound(srv)
        .metrics_flush_interval(Duration::from_secs(5))
        .run();
    let client = client::new(proxy.inbound, "test.conduit.local");

    info!("client.get(/hey)");
    assert_eq!(client.get("/hey"), "hello");

    info!("client.get(/hi)");
    assert_eq!(client.get("/hi"), "good morning");
    assert_eq!(client.get("/hi"), "good morning");

    info!("awaiting report");
    let report = reports.wait().next().unwrap().unwrap();

    // requests -----------------------
    assert_eq!(report.requests.len(), 2);
    // first request
    let req = &report.requests[0];
    assert_eq!(req.ctx.as_ref().unwrap().authority, "test.conduit.local");
    assert_eq!(req.ctx.as_ref().unwrap().path, "/hey");
    let res = &req.responses[0];
    assert_eq!(res.response_latencies.len(), 25);

    for ref bucket in &res.response_latencies {
        // 500 ms of extra latency should put us in the 500-10000
        // decimillisecond bucket.
        if bucket.latency == 10000 {
            assert_eq!(bucket.count, 1);
        } else {
            assert_eq!(bucket.count, 0);
        }
    }

    // second request
    let req = &report.requests.get(1).expect("second report");
    assert_eq!(req.ctx.as_ref().unwrap().authority, "test.conduit.local");
    assert_eq!(req.ctx.as_ref().unwrap().path, "/hi");
    assert_eq!(req.count, 2);
    assert_eq!(req.responses.len(), 1);
    let res = req.responses.get(0).expect("responses[0]");
    assert_eq!(res.response_latencies.len(), 25);
    for ref bucket in &res.response_latencies {
        // 40 ms of extra latency should put us in the 400-500
        // decimillisecond bucket.
        if bucket.latency == 500 {
            assert_eq!(bucket.count, 2);
        } else {
            assert_eq!(bucket.count, 0);
        }
    }

}

#[test]
fn telemetry_report_errors_are_ignored() {}
