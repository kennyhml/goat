use std::{env, error::Error, io};

use zadt::{Client, Operation, QueryTransportKind, ReqwestTransport, TransportsQuery};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let transport = ReqwestTransport::builder()
        .destination(required_env("SAP_DESTINATION")?)
        .sap_client(required_env("SAP_CLIENT")?)
        .language(env::var("SAP_LANGUAGE").unwrap_or_else(|_| "EN".to_owned()))
        .basic_auth(required_env("SAP_USERNAME")?, required_env("SAP_PASSWORD")?)
        .build()?;
    let client = Client::new(transport).discover().await?;
    let transports = TransportsQuery::builder()
        .kind(QueryTransportKind::All)
        .build()?
        .execute(&client)
        .await?;

    for request in transports.requests {
        println!(
            "{}\t{}\t{}\t{} {}\t{}",
            request.number,
            request.kind,
            request.status,
            request.date,
            request.time,
            request.description
        );
    }

    Ok(())
}

fn required_env(name: &str) -> Result<String, io::Error> {
    env::var(name).map_err(|source| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing required environment variable `{name}`: {source}"),
        )
    })
}
