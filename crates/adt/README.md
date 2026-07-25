# goat-adt

Typed SAP ABAP Development Tools protocol support for the `goat` framework.

Cleaner rewrite of [adt-query](https://github.com/kennyhml/adt-query) to better make use of HATEOAS.

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
