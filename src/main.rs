use std::{env, error::Error, io, time::Duration};

use tokio::time::sleep;
use zadt::{Client, Logon, Operation, Package, Program, ProgramProperties, ReqwestTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

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
        .build()?;
    let client = Client::new(transport);
    Logon.execute(&client).await?;
    let client = client.discover().await?;

    let pkg = client.object::<Package>("/DMO/FLIGHT")?;

    let session = client.create_user_session();

    // let lock = pkg.lock(zadt::AccessMode::Modify).execute(&session).await?;
    //
    // sleep(Duration::from_secs(10)).await;
    //
    // lock.remove().execute(&session).await?;

    let program = client.object::<Program>("z_test")?;
    let response = program.query().execute(&client).await?;
    let properties = match response {
        ProgramProperties::V2(properties) | ProgramProperties::V3(properties) => properties,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "unsupported program-properties representation",
            )
            .into());
        }
    };

    let source = properties.source.query().execute(&client).await?;

    println!("program: {}", properties.reference.name());
    println!("description: {}", properties.description);
    println!(
        "package: {} ({})",
        properties.package.name(),
        properties.package.uri()
    );
    println!("version: {:?}", properties.version);
    println!("\n{}", source.content);

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
