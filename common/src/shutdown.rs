use owo_colors::OwoColorize;

pub async fn shutdown_signal() {
    // Listen for both SIGINT (Ctrl+C) and SIGTERM (K8s)
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
        let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");

        tokio::select! {
            _ = sigint.recv()  => eprintln!("{}", "🛑 Received SIGINT".red()),
            _ = sigterm.recv() => eprintln!("{}", "🛑 Received SIGTERM".red()),
        }
    }

    #[cfg(not(unix))]
    {
        // Fallback: only Ctrl+C on non-Unix
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl+C handler");
    }
}
