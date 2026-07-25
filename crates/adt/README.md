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
directly through `Client`; operations requiring a persistent SAP user session
are represented separately as stateful operations.

### SAP session terminology

| Term | Representation | Purpose |
| --- | --- | --- |
| User context | `sap-usercontext` | Selects the SAP client and language. |
| HTTP security session | `SAP_SESSIONID_*` and related cookies | Retains authenticated HTTP session state shared by requests from one transport. |
| User session | `sap-contextid` | Retains stateful ABAP execution state across requests. Active sessions can be inspected in transaction `SM04`. |

A user session is used for workflows such as editing a program, where a lock
must remain valid across multiple requests. It is bound to the HTTP security
session but has its own identity and lifecycle.

`ReqwestTransport` sends the configured SAP client and language in the
`sap-usercontext` cookie. They are intentionally not appended to every resource
URI: some ADT handlers, including core discovery on tested systems, interpret
all query parameters as operation-specific input.

The transport retains response cookies in an RFC-aware, destination-scoped
cookie store, allowing stateless operations to reuse the same SAP HTTP security
session. It deliberately excludes `sap-contextid`: that cookie identifies one
SAP user session and is owned by the corresponding `UserSession`
rather than shared across every request.

A `UserSession` owns a cheap clone of its client, with discovered
capabilities shared through `Arc`. It therefore has no borrowing lifetime and
can be stored for an entire editing workflow. Requests through one session are
serialized and carry its latest `sap-contextid`; separate user sessions retain
independent context IDs while sharing the client's HTTP security session.
Call `UserSession::close()` when the workflow finishes. Dropping an instance
only releases local state and does not notify SAP.

## Resource references

Resource references separate validated ADT locations from the operations that
act on them:

| Type | Represents | Created from |
| --- | --- | --- |
| `ObjectRef` | A lockable repository object | A validated `AdtUri`, a parsed URI, or a domain reference such as `ProgramRef` |
| `SourceRef` | One source resource plus its owning object | `ObjectRef::main_source()` or `ObjectRef::source(...)` |
| `ProgramRef` | A program object resolved through discovery | `Client<Discovered>::program(name)` |

Constructing a reference performs no request. For a known object URI:

```rust
use goat_adt::{AccessMode, ObjectRef};

# fn example() -> Result<(), Box<dyn std::error::Error>> {
let structure = ObjectRef::parse(
    "/sap/bc/adt/ddic/structures/ZSTRUCTURE",
)?;
let source = structure.main_source();
let lock_operation = structure.lock(AccessMode::Modify);

assert_eq!(&source.object, &structure);
assert_eq!(
    source.uri.as_str(),
    "/sap/bc/adt/ddic/structures/ZSTRUCTURE/source/main",
);
let _ = lock_operation;
# Ok(())
# }
```

Keeping the owning `ObjectRef` inside `SourceRef` lets the update builder reject
a `LockHandle` obtained for a different object before any request is sent.

## Object editing

Object locking and source updates are generic stateful operations. A
`ProgramRef` resolves its object and source resources from central discovery:

```rust,ignore
use goat_adt::{AccessMode, Operation};

let session = client.create_user_session();
let program = client.program("ZDEMO")?;
let lock_handle = program
    .lock(AccessMode::Modify)
    .execute(&session)
    .await?;

program
    .source()
    .update()
    .lock_handle(lock_handle.clone())
    .content("REPORT zdemo.\n")
    .build()?
    .execute(&session)
    .await?;

program.unlock(lock_handle)?.execute(&session).await?;
session.close().await?;
```

When the owning resource is not otherwise needed, the equivalent lock-owned
form is `lock_handle.remove().execute(&session).await?`.

`ObjectRef::lock()` constructs an `ObjectLock` operation, while
`SourceRef::update()` seeds an `ObjectSourceUpdateBuilder` with its validated
source. `ObjectLock` parses SAP's opaque `LOCK_HANDLE` into a `LockHandle`; the
update builder rejects a handle obtained for another object.
The same operations can therefore edit programs, classes, DDIC structures, CDS
sources, and other source-backed repository objects without duplicating their
stateful protocol.

The `UserSession` serializes both calls and carries the `sap-contextid` returned
by the lock response into the update request. For example, a structure can use
`ObjectRef::parse("/sap/bc/adt/ddic/structures/ZSTRUCTURE")?` and its
`main_source()` with the same lock and update builders.

The active development-handler diagnostics map both operations to
`CL_SEDI_ADT_RES_PROGRAM`: method `POST` handles the lock and method `PUT`
handles the source update. The routes are registered by
`CL_SEDI_ADT_RES_APP_PROGRAMS->REGISTER_RESOURCES`. These class names are
implementation details and are not part of the client contract.

The complete workflow is covered by a mock HTTP test and has also been verified
against the active SAP backend with `Z_TEST`: the client fetched its source,
locked it, appended a test comment, updated it, unlocked it, and closed the
user session. The updated source was fetched again to verify the change. The
HTTP transport acquires and caches a CSRF token before mutating requests. Retry
after a stale CSRF token is not implemented.
