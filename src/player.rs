use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// mpv IPC soketine bağlan, makul I/O timeout'larını uygula.
fn connect(sock: &str) -> Option<UnixStream> {
    const TIMEOUT: Duration = Duration::from_millis(400);
    let stream = UnixStream::connect(sock).ok()?;
    stream.set_read_timeout(Some(TIMEOUT)).ok();
    stream.set_write_timeout(Some(TIMEOUT)).ok();
    Some(stream)
}

/// Yeni satırla biten tek mpv komutu yaz ve `data` alanını döndür.
/// mpv `error != "success"` verirse `None`.
fn request(stream: &mut UnixStream, cmd: &str) -> Option<serde_json::Value> {
    let mut line = String::with_capacity(cmd.len() + 1);
    line.push_str(cmd);
    line.push('\n');
    stream.write_all(line.as_bytes()).ok()?;

    let mut buf = [0u8; 512];
    let n = stream.read(&mut buf).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&buf[..n]).ok()?;
    if v["error"].as_str() != Some("success") {
        return None;
    }
    Some(v["data"].clone())
}

/// `get_property` cevabını sayıya indir (bool → 1.0/0.0).
fn get_property(stream: &mut UnixStream, prop: &str) -> Option<f64> {
    let data = request(stream, &format!("{{\"command\":[\"get_property\",\"{prop}\"]}}"))?;
    match data {
        serde_json::Value::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
        other => other.as_f64(),
    }
}

pub fn query_mpv_prop(sock: &str, prop: &str) -> Option<f64> {
    get_property(&mut connect(sock)?, prop)
}

pub fn query_mpv_position(sock: &str) -> Option<(f64, f64)> {
    let mut stream = connect(sock)?;
    let pos = get_property(&mut stream, "time-pos")?;
    let dur = get_property(&mut stream, "duration").unwrap_or(0.0);
    Some((pos, dur))
}

pub fn send_mpv_cmd(sock: &str, json_cmd: &str) -> bool {
    let Some(mut stream) = connect(sock) else { return false; };
    stream.write_all(json_cmd.as_bytes()).is_ok()
}
