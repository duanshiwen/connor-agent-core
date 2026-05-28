use agentos_sidecar::pb::identity_core_service_server::IdentityCoreServiceServer;
use agentos_sidecar::pb::sidecar_health_service_server::SidecarHealthServiceServer;
use agentos_sidecar::{HealthService, IdentityService};
use std::path::Path;
use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tonic::transport::Server;

const DEFAULT_SOCKET: &str = "/tmp/agentos-sidecar.sock";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let socket_path = parse_socket_arg();
    if let Some(parent) = Path::new(&socket_path).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if Path::new(&socket_path).exists() {
        tokio::fs::remove_file(&socket_path).await?;
    }

    let uds = UnixListener::bind(&socket_path)?;
    let incoming = UnixListenerStream::new(uds);

    println!("agentos-sidecar listening on unix://{}", socket_path);

    Server::builder()
        .add_service(SidecarHealthServiceServer::new(HealthService))
        .add_service(IdentityCoreServiceServer::new(IdentityService::new()))
        .serve_with_incoming(incoming)
        .await?;

    Ok(())
}

fn parse_socket_arg() -> String {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--socket" {
            if let Some(value) = args.next() {
                return value;
            }
        }
        if let Some(value) = arg.strip_prefix("--socket=") {
            return value.to_string();
        }
    }
    std::env::var("AGENTOS_SIDECAR_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET.to_string())
}
