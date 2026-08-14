use std::net::TcpListener;

pub fn pick_free_port_127() -> std::io::Result<u16> {
    let l = TcpListener::bind(("127.0.0.1", 0))?;
    let p = l.local_addr()?.port();
    drop(l);
    Ok(p)
}
