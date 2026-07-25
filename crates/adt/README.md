# goat-adt

Typed SAP ABAP Development Tools protocol support for the `goat` framework.

The crate currently provides:

- a typestate client that loads and retains central ADT capabilities;
- typed operations for core and central discovery;
- transport-neutral ADT requests and responses;
- an HTTP transport with Basic authentication; and
- parsed workspaces, collections, categories, accepted media types, and URI
  template links.

## Quick start

Create a transport and run central discovery:

```rust,no_run
use goat_adt::{Client, ReqwestTransport};

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let transport = ReqwestTransport::builder()
    .destination("https://sap.example.test")
    .sap_client("001")
    .language("EN")
    .basic_auth("DEVELOPER", "secret")
    .build()?;

let client = Client::new(transport).discover().await?;

let _programs = client.capabilities().collection(
    "http://www.sap.com/adt/categories/programs",
    "programs",
);
# Ok(())
# }
```

`Client::discover()` consumes the `Client<Undiscovered>` and returns a
`Client<Discovered>`. The discovered client retains the central capability
document for subsequent operations.

Collections should be selected by their category `scheme` and `term`, as in
the example, rather than by title. Titles are display text and can be
localized; the category pair is the stable protocol identity.

## Discovery endpoints

ADT exposes two AtomPub service documents with different roles:

| Operation | Endpoint | Role |
| --- | --- | --- |
| `CoreDiscoveryQuery` | `/sap/bc/adt/core/discovery` | Small bootstrap document advertising infrastructure such as compatibility and batch resources. |
| `DiscoveryQuery` | `/sap/bc/adt/discovery` | Central document advertising domain workspaces and collections such as programs. |

Both operations have fixed URIs and can execute with either client state.
`CoreDiscoveryQuery` returns its capabilities without changing the client state.
`Client::discover()` specifically executes `DiscoveryQuery` and stores its
result while transitioning the client to `Discovered`.

Discovery is the top-level capability map, not a complete description of
every ADT interaction. Later resource representations can advertise
resource-specific links, and operation-specific request and response formats
remain part of their media-type contracts.

## Server-side handlers

SAP's ADT development diagnostics map a resource URI to the ABAP application
that registered it and to the resource method that serves it:

```text
GET /sap/bc/adt/development/handler/application?uri=<ADT URI>
GET /sap/bc/adt/development/handler/adtresource?uri=<ADT URI>&method=GET
```

The following handlers were observed for the discovery endpoints:

| Endpoint | Registration handler | GET resource handler |
| --- | --- | --- |
| `/sap/bc/adt/core/discovery` | `CL_ADT_DISCOVERY_BASE_RES_APP->REGISTER_RESOURCES` | `CL_ADT_RES_DISCOVERY_BASE->GET` |
| `/sap/bc/adt/discovery` | `CL_ADT_DISCOVERY_RES_APP->REGISTER_RESOURCES` | `CL_ADT_RES_DISCOVERY->GET` |

The diagnostic responses link directly to the corresponding class source
under `/sap/bc/adt/oo/classes/.../source/main`. These class and method names
are useful when investigating server behavior, but they are SAP
implementation details and may differ by release. The development diagnostic
endpoints can also be unavailable or unauthorized on production systems.
`goat-adt` depends on the discovery wire contract, not these implementation
class names.

## Execution model

Operations produce transport-neutral ADT requests. `ReqwestTransport` is the
default HTTP implementation; other HTTP clients and future RFC-backed
transports can implement the `Transport` trait. Stateless operations execute
directly through `Client`; operations requiring a persistent ADT user context
are represented separately as stateful operations.

`ReqwestTransport` sends the configured SAP client and language in the
`sap-usercontext` cookie. They are intentionally not appended to every resource
URI: some ADT handlers, including core discovery on tested systems, interpret
all query parameters as operation-specific input.
