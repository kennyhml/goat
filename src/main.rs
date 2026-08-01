use std::{env, error::Error, io, time::Duration};

use tokio::time::sleep;
use tracing_subscriber::EnvFilter;
use zadt::{
    Client, Logon, Operation, Package, Program, ProgramProperties, RepositoryContent,
    RepositoryContentQuery, RepositoryFacet, RepositoryObjectPropertiesQuery,
    RepositoryPreselection, ReqwestTransport, TransportExt,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .pretty()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("zadt=debug")),
        )
        .init();

    let destination = required_env("SAP_DESTINATION")?;
    let sap_client = required_env("SAP_CLIENT")?;
    let username = required_env("SAP_USERNAME")?;
    let password = required_env("SAP_PASSWORD")?;
    let language = env::var("SAP_LANGUAGE").unwrap_or_else(|_| "EN".to_owned());

    let transport = ReqwestTransport::builder()
        .destination(destination)
        .sap_client(sap_client)
        .language(language)
        .basic_auth(username, password)
        .build()?
        .traced();
    let client = Client::new(transport);
    Logon.execute(&client).await?;
    let client = client.discover().await?;

    let res = RepositoryContentQuery::builder()
        .preselection(RepositoryPreselection::new(
            RepositoryFacet::PACKAGE,
            "$TMP",
        ))
        .preselection(RepositoryPreselection::new(
            RepositoryFacet::OWNER,
            "DEVELOPER",
        ))
        .preselection(RepositoryPreselection::new(
            RepositoryFacet::GROUP,
            "SOURCE_LIBRARY",
        ))
        .preselection(RepositoryPreselection::new(RepositoryFacet::TYPE, "CLAS").include("PROG"))
        .build()?
        .execute(&client)
        .await?;

    println!("{:#?}", res);

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
