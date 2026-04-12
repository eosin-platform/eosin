use std::net::SocketAddr;

use http_body_util::Full;
use hyper::{Request, Response, body::Bytes, header::CONTENT_TYPE, service::service_fn};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder;
use lazy_static::lazy_static;
use owo_colors::OwoColorize;
use prometheus::{Counter, Encoder, Gauge, HistogramVec, TextEncoder};
use prometheus::{labels, opts, register_counter, register_gauge, register_histogram_vec};
use tokio::net::TcpListener;

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
async fn serve_req(
    _req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, hyper::Error> {
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
        .body(Full::new(Bytes::from(buffer)))
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
    let listener = TcpListener::bind(addr)
        .await
        .expect("failed to bind metrics server");
    loop {
        let (stream, _) = listener
            .accept()
            .await
            .expect("failed to accept connection");
        let io = TokioIo::new(stream);
        tokio::spawn(async move {
            if let Err(err) = Builder::new(TokioExecutor::new())
                .serve_connection(io, service_fn(serve_req))
                .await
            {
                eprintln!("metrics server connection error: {}", err);
            }
        });
    }
}
