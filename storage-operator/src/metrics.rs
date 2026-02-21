use std::net::SocketAddr;

use hyper::{
    Body, Request, Response, Server,
    header::CONTENT_TYPE,
    service::{make_service_fn, service_fn},
};
use lazy_static::lazy_static;
use owo_colors::OwoColorize;
use prometheus::{Counter, Encoder, Gauge, HistogramVec, TextEncoder};
use prometheus::{labels, opts, register_counter, register_gauge, register_histogram_vec};

use crate::util::metrics::prefix;

lazy_static! {
    static ref HTTP_COUNTER: Counter = register_counter!(opts!(
        &format!("{}_http_requests_total", prefix()),
        "Number of HTTP requests made to the metrics server.",
        labels! {"handler" => "all",}
    ))
    .unwrap();
    static ref HTTP_BODY_GAUGE: Gauge = register_gauge!(opts!(
        &format!("{}_http_response_size_bytes", prefix()),
        "Metrics server HTTP response sizes in bytes.",
        labels! {"handler" => "all",}
    ))
    .unwrap();
    static ref HTTP_REQ_HISTOGRAM: HistogramVec = register_histogram_vec!(
        &format!("{}_http_request_duration_seconds", prefix()),
        "Metrics server HTTP request latencies in seconds.",
        &["handler"]
    )
    .unwrap();
}

/// Handler to serve the prometheus metrics to the request.
async fn serve_req(_req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let encoder = TextEncoder::new();
    HTTP_COUNTER.inc();
    let timer = HTTP_REQ_HISTOGRAM.with_label_values(&["all"]).start_timer();
    let metric_families = prometheus::gather();
    let mut buffer = vec![];
    encoder.encode(&metric_families, &mut buffer).unwrap();
    HTTP_BODY_GAUGE.set(buffer.len() as f64);
    let response = Response::builder()
        .status(200)
        .header(CONTENT_TYPE, encoder.format_type())
        .body(Body::from(buffer))
        .unwrap();
    timer.observe_duration();
    Ok(response)
}

/// Runs the prometheus metrics server on the given port.
pub async fn run_server(port: u16) {
    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    println!(
        "{}{}",
        "📈 Metrics server listening on ".green(),
        addr.to_string().green().dimmed()
    );
    let serve_future = Server::bind(&addr).serve(make_service_fn(|_| async {
        Ok::<_, hyper::Error>(service_fn(serve_req))
    }));
    if let Err(err) = serve_future.await {
        panic!("metrics server error: {}", err);
    }
    panic!("metrics server exited");
}
