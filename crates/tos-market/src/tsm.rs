/// Published session-manager host. A session file may name another in
/// `TOS_TSM_URL`. Saving a session keeps that cookie; this crate does not
/// call the host, and it does not switch this session onto the live gateway.
pub const TSM_URL: &str = "https://tosweb-sm.thinkorswim.com/api/v1/tsm";

/// `Cookie` header from DevTools cookies. Names or values that could break
/// the header are dropped.
pub(crate) fn cookie_header<'a>(cookies: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
    let mut out = String::new();
    for (name, value) in cookies {
        if name.is_empty()
            || name.contains([';', ' ', '\r', '\n', '='])
            || value.contains(['\r', '\n', ';'])
        {
            continue;
        }
        if !out.is_empty() {
            out.push_str("; ");
        }
        out.push_str(name);
        out.push('=');
        out.push_str(value);
    }
    out
}
