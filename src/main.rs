use std::{env, error::Error, io, time::Duration};

use tokio::time::sleep;
use tracing_subscriber::EnvFilter;
use zadt::{
    Client, Logon, Operation, Package, PackageSettingsQuery, Program, ProgramProperties,
    RepositoryContent, RepositoryContentQuery, RepositoryFacet, RepositoryObjectPropertiesQuery,
    RepositoryPreselection, ReqwestTransport, TransportExt,
};
use zvfs::{FacetLevel, FacetPolicy, Mount, VirtualRepositoryTree};

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
        .traced()
        .with_body_logging(64 * 1024);
    let client = Client::new(transport).discover().await?;

    let package = client.object::<Package>("$TMP")?;
    let tree = package.sub_tree().execute(&client).await?;

    for node in tree.nodes {
        println!(
            "{}: children={}, interfaces={}",
            node.package.reference.name(),
            node.has_subpackages,
            node.has_interfaces,
        );
    }

    // let properties = package.query().execute(&client).await?;
    // let ancestors = package.super_tree().execute(&client).await?;
    // let children = package.sub_tree().execute(&client).await?;
    // let settings = PackageSettingsQuery.execute(&client).await?;
    // println!("{:#?}", properties);

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
