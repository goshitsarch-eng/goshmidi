//! Per-profile local activation. OS file locking prevents stale PID decisions.
use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "linux"))]
use std::net::{SocketAddr, TcpListener as Listener, TcpStream as Stream};
#[cfg(target_os = "linux")]
use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::Duration,
};
#[derive(Serialize, Deserialize)]
struct Endpoint {
    #[cfg(not(target_os = "linux"))]
    address: SocketAddr,
    #[cfg(target_os = "linux")]
    address: PathBuf,
    token: String,
}
#[derive(Serialize, Deserialize)]
struct Activation {
    token: String,
    files: Vec<PathBuf>,
}

pub enum Instance {
    Primary(LocalInstance),
    Forwarded,
}
pub struct LocalInstance {
    _lock: File,
    receiver: mpsc::Receiver<Vec<PathBuf>>,
    running: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    endpoint: PathBuf,
    #[cfg(target_os = "linux")]
    socket: PathBuf,
}
impl LocalInstance {
    pub fn take(&self) -> Option<Vec<PathBuf>> {
        self.receiver.try_recv().ok()
    }
}
impl Drop for LocalInstance {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let _ = std::fs::remove_file(&self.endpoint);
        #[cfg(target_os = "linux")]
        let _ = std::fs::remove_file(&self.socket);
    }
}
pub fn activate(config_file: &Path, files: Vec<PathBuf>) -> Result<Instance, String> {
    let parent = config_file.parent().unwrap_or_else(|| Path::new("."));
    let dir = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Could not create instance directory: {e}"))?;
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut lock_name = config_file
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("dmidiplayer.conf"))
        .to_os_string();
    let mut endpoint_name = lock_name.clone();
    lock_name.push(".instance.lock");
    endpoint_name.push(".instance.json");
    let lock = options
        .open(dir.join(lock_name))
        .map_err(|e| e.to_string())?;
    let endpoint = dir.join(endpoint_name);
    match lock.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            for _ in 0..20 {
                let result: Result<(), String> = (|| {
                    let endpoint: Endpoint = serde_json::from_slice(
                        &std::fs::read(&endpoint).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    #[cfg(not(target_os = "linux"))]
                    if !endpoint.address.ip().is_loopback() {
                        return Err("Instance endpoint is not local".into());
                    }
                    #[cfg(target_os = "linux")]
                    let mut stream =
                        Stream::connect(&endpoint.address).map_err(|e| e.to_string())?;
                    #[cfg(not(target_os = "linux"))]
                    let mut stream =
                        Stream::connect_timeout(&endpoint.address, Duration::from_millis(200))
                            .map_err(|e| e.to_string())?;
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .map_err(|e| e.to_string())?;
                    stream
                        .set_write_timeout(Some(Duration::from_secs(1)))
                        .map_err(|e| e.to_string())?;
                    let data = serde_json::to_vec(&Activation {
                        token: endpoint.token,
                        files: files.clone(),
                    })
                    .map_err(|e| e.to_string())?;
                    if data.len() > 64 * 1024 {
                        return Err("Too many files for activation".into());
                    }
                    stream.write_all(&data).map_err(|e| e.to_string())?;
                    stream
                        .shutdown(std::net::Shutdown::Write)
                        .map_err(|e| e.to_string())?;
                    let mut response = [0; 2];
                    stream
                        .read_exact(&mut response)
                        .map_err(|e| e.to_string())?;
                    if response != *b"OK" {
                        return Err("Existing instance rejected activation".into());
                    }
                    Ok(())
                })();
                if result.is_ok() {
                    return Ok(Instance::Forwarded);
                }
                thread::sleep(Duration::from_millis(50));
            }
            return Err(
                "The current instance is not responding. Close it before reopening this profile."
                    .into(),
            );
        }
        Err(error) => return Err(format!("Could not lock application profile: {error}")),
    }
    #[cfg(target_os = "linux")]
    let socket = {
        use std::os::unix::fs::FileTypeExt;
        let hash = config_file
            .file_name()
            .unwrap_or_default()
            .as_encoded_bytes()
            .iter()
            .fold(0xcbf29ce484222325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
            });
        let socket = dir.join(format!(".gosh-{hash:016x}.sock"));
        if let Ok(metadata) = std::fs::symlink_metadata(&socket) {
            if !metadata.file_type().is_socket() {
                return Err("Activation socket path is occupied by another file".into());
            }
            std::fs::remove_file(&socket).map_err(|e| e.to_string())?;
        }
        socket
    };
    #[cfg(target_os = "linux")]
    let listener = Listener::bind(&socket)
        .map_err(|e| format!("Could not create local activation socket: {e}"))?;
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(target_os = "linux"))]
    let listener = Listener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .map_err(|e| format!("Could not create local activation service: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let mut entropy = [0u8; 32];
    getrandom::fill(&mut entropy).map_err(|e| e.to_string())?;
    let token = entropy
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let record = Endpoint {
        #[cfg(not(target_os = "linux"))]
        address: listener.local_addr().map_err(|e| e.to_string())?,
        #[cfg(target_os = "linux")]
        address: socket.clone(),
        token: token.clone(),
    };
    super::persistence::write_atomic(
        &endpoint,
        serde_json::to_vec(&record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let (sender, receiver) = mpsc::channel();
    let running = Arc::new(AtomicBool::new(true));
    let worker_running = running.clone();
    let worker = thread::spawn(move || {
        while worker_running.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _address)) => {
                    #[cfg(not(target_os = "linux"))]
                    if !_address.ip().is_loopback() {
                        continue;
                    }
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(250)));
                    let mut bytes = Vec::new();
                    if (&mut stream)
                        .take(64 * 1024 + 1)
                        .read_to_end(&mut bytes)
                        .is_err()
                        || bytes.len() > 64 * 1024
                    {
                        continue;
                    }
                    if let Ok(activation) = serde_json::from_slice::<Activation>(&bytes)
                        && activation.token == token
                        && sender.send(activation.files).is_ok()
                    {
                        let _ = stream.write_all(b"OK");
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(25))
                }
                Err(error) => {
                    tracing::error!(%error,"Activation listener failed");
                    break;
                }
            }
        }
    });
    Ok(Instance::Primary(LocalInstance {
        _lock: lock,
        receiver,
        running,
        worker: Some(worker),
        endpoint,
        #[cfg(target_os = "linux")]
        socket,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_activation_and_stale_endpoint_recovery() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("app.conf");
        let Instance::Primary(primary) = activate(&config, vec![]).unwrap() else {
            panic!("Expected primary");
        };
        let endpoint: Endpoint =
            serde_json::from_slice(&std::fs::read(&primary.endpoint).unwrap()).unwrap();
        let mut unauthenticated = Stream::connect(endpoint.address).unwrap();
        unauthenticated
            .write_all(br#"{"token":"wrong","files":["bad.mid"]}"#)
            .unwrap();
        unauthenticated.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = Vec::new();
        unauthenticated.read_to_end(&mut response).unwrap();
        assert!(response.is_empty());
        assert!(primary.take().is_none());
        let files = vec![PathBuf::from("Unicode 音楽.mid")];
        assert!(matches!(
            activate(&config, files.clone()).unwrap(),
            Instance::Forwarded
        ));
        assert_eq!(primary.take(), Some(files));
        let other = activate(&temp.path().join("other.conf"), vec![]).unwrap();
        assert!(matches!(other, Instance::Primary(_)));
        let endpoint_path = primary.endpoint.clone();
        drop(primary);
        assert!(!endpoint_path.exists());
        std::fs::write(&endpoint_path, b"stale").unwrap();
        assert!(matches!(
            activate(&config, vec![]).unwrap(),
            Instance::Primary(_)
        ));
    }
}
