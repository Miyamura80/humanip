//! Process metrics. Contract stub: the ops workstream implements counters
//! and Prometheus text rendering; the server calls these hooks.

/// Front-end protocol of a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proto {
    Socks5,
    HttpConnect,
    HttpForward,
}

/// How a connection ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnResult {
    Ok,
    AuthFailed,
    Denied,
    Blocked,
    DialFailed,
    ProtocolError,
}

pub fn connection_opened(_proto: Proto) {}
pub fn connection_closed(_proto: Proto, _result: ConnResult) {}
pub fn bytes(_provider: &str, _up: u64, _down: u64) {}
pub fn dial_failure(_provider: &str) {}

/// Prometheus text exposition of all metrics.
pub fn render() -> String {
    String::new()
}
