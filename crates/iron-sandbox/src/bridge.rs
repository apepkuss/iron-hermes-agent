/// Generate a Python bridge script that stubs sandbox-whitelisted tools via UDS JSON RPC.
pub fn generate_python_bridge(socket_path: &str, tools: &[&str]) -> String {
    let socket_repr = python_repr(socket_path);
    let mut script = format!(
        r#"import socket
import json
import sys

_SOCKET_PATH = {socket_repr}

def _call(tool, args):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        sock.connect(_SOCKET_PATH)
        payload = json.dumps({{"tool": tool, "args": args}}) + "\n"
        sock.sendall(payload.encode("utf-8"))
        buf = b""
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            buf += chunk
            if b"\n" in buf:
                break
        response = json.loads(buf.split(b"\n")[0].decode("utf-8"))
        return response
    finally:
        sock.close()

"#
    );

    for tool in tools {
        let fn_name = tool.replace('-', "_");
        let tool_repr = python_repr(tool);
        script.push_str(&format!(
            "def {fn_name}(**kwargs):\n    return _call({tool_repr}, kwargs)\n\n"
        ));
    }

    script
}

/// Generate a Shell bridge script that wraps UDS calls using python3.
pub fn generate_shell_bridge(socket_path: &str, tools: &[&str]) -> String {
    // Shell-escape the socket path by wrapping in single quotes (replacing ' with '\'')
    let escaped = shell_single_quote(socket_path);
    let mut script = format!(
        r#"#!/bin/sh
# Iron Sandbox Shell Bridge
_SANDBOX_SOCKET={escaped}

_call_tool() {{
    local tool="$1"
    local args="$2"
    printf '%s\n' "{{\\"tool\\": \\"$tool\\", \\"args\\": $args}}" | python3 -c "
import socket, json, sys
data = sys.stdin.read().strip()
sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
sock.connect('$_SANDBOX_SOCKET')
sock.sendall((data + chr(10)).encode())
buf = b''
while True:
    chunk = sock.recv(4096)
    if not chunk:
        break
    buf += chunk
    if b'\n' in buf:
        break
print(buf.split(b'\n')[0].decode())
sock.close()
"
}}

"#
    );

    for tool in tools {
        let fn_name = tool.replace('-', "_");
        let escaped_tool = shell_single_quote(tool);
        script.push_str(&format!(
            "{fn_name}() {{\n    _call_tool {escaped_tool} \"${{1:-{{}}}}\"\n}}\n\n"
        ));
    }

    script
}

/// Wrap a string in Python single quotes, escaping backslashes and single quotes.
fn python_repr(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('\'', "\\'");
    format!("'{escaped}'")
}

/// Wrap a string in shell single quotes (replace ' with '\'').
fn shell_single_quote(s: &str) -> String {
    let escaped = s.replace('\'', "'\\''");
    format!("'{escaped}'")
}

/// The set of tools available inside the sandbox.
pub const SANDBOX_TOOL_WHITELIST: &[&str] = &[
    "web_search",
    "web_extract",
    "read_file",
    "write_file",
    "search_files",
    "patch",
    "terminal",
];
