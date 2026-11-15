//! The systems a Rithmic gateway offers, asked before anyone signs in.
//!
//! Every Rithmic system (Rithmic 01, Rithmic Paper Trading, a prop firm's
//! own such as Tradeify) is reached through the same gateways, and a login
//! belongs to exactly one of them. A gateway names them to whoever asks,
//! with no user or password: the ticker plant is dialled, asked, and let go,
//! and nothing is signed in to.

use rithmic_rs::rti::messages::RithmicMessage;
use rithmic_rs::{ConnectStrategy, RithmicTickerPlant};

use crate::client::{config, within, REQUEST};
use crate::{App, Credentials, Error};

/// The systems the gateway at `url` offers, in the order it names them.
///
/// Blocks for as long as one dial and one request take, at most a few
/// seconds each, so it belongs off the GTK thread.
pub fn systems(app: App, url: &str) -> Result<Vec<String>, Error> {
    crate::connection::tls();
    // The builder asks for a login, which is never sent: nothing here signs
    // in.
    let nobody = Credentials {
        user: "nobody".into(),
        password: String::new(),
        system: crate::TEST_SYSTEM.into(),
        url: url.into(),
    };
    let config = config(app, &nobody)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| Error::Connection(error.to_string()))?;
    runtime.block_on(async {
        let plant = within(REQUEST, RithmicTickerPlant::connect(&config, ConnectStrategy::Simple)).await?;
        let handle = plant.get_handle();
        let answer = within(REQUEST, handle.get_system_info()).await;
        handle.abort();
        match answer?.message {
            RithmicMessage::ResponseRithmicSystemInfo(info) => Ok(info.system_name),
            other => Err(Error::Connection(format!("the gateway answered {other:?}"))),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A gateway that hangs up names no systems, and says so rather than
    /// waiting. The gateway is a local socket: nothing leaves this machine.
    #[test]
    fn a_gateway_that_hangs_up_names_none() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a local port");
        let port = listener.local_addr().unwrap().port();
        let gateway = std::thread::spawn(move || drop(listener.accept()));
        let app = App { name: "test", version: "0" };
        assert!(systems(app, &format!("wss://localhost:{port}")).is_err());
        gateway.join().unwrap();
    }
}
