use std::{env, error::Error, io};

use tracing_subscriber::EnvFilter;
use zadt::{Client, RepositoryFacet, RepositoryPreselection, ReqwestTransport, TransportExt};
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

    let preselections = vec![
        RepositoryPreselection::new(RepositoryFacet::PACKAGE, "$TMP"),
        RepositoryPreselection::new(RepositoryFacet::OWNER, "DEVELOPER").include("DDIC"),
    ];

    let tree = VirtualRepositoryTree::builder(client.clone())
        .mount(
            Mount::selection("Local Objects", preselections).facet_policy(FacetPolicy::new([
                FacetLevel::always(RepositoryFacet::OWNER),
                FacetLevel::always(RepositoryFacet::GROUP),
                FacetLevel::adaptive(RepositoryFacet::TYPE, 2),
            ])),
        )
        .build()
        .await?;

    let res = tree.children(tree.root()).await.unwrap();

    let mount_children = tree.children(res[0].id).await.unwrap();
    // for i in 0..mount_children.len() {
    let dev = tree.children(mount_children[1].id).await?;

    tree.children(dev[2].id).await?;
    // }
    println!("{}", tree.render_tree());

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
