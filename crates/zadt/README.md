# zadt

Typed SAP ABAP Development Tools protocol support for the Ziege framework.

Cleaner rewrite of [adt-query](https://github.com/kennyhml/adt-query) to better make use of HATEOAS.

## Discovery endpoints
ADT exposes two AtomPub service documents with different roles:

| Operation | Endpoint | Role |
| --- | --- | --- |
| `CoreDiscoveryQuery` | `/sap/bc/adt/core/discovery` | Small bootstrap document advertising infrastructure such as compatibility and batch resources. |
| `DiscoveryQuery` | `/sap/bc/adt/discovery` | Central document advertising domain workspaces and collections such as programs. |

Both operations have fixed URIs and can execute before central discovery.
`CoreDiscoveryQuery` returns its capabilities without changing the client state.
`Client::new()` creates an `Initial` client, and `Client::discover()` executes
`DiscoveryQuery` and stores its result while transitioning the client to `Ready`.
HTTP security-session establishment remains an explicit `Logon` operation and
does not become client typestate.

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
`zadt` depends on the discovery wire contract, not these implementation
class names.

## Execution model
Operations produce transport-neutral ADT requests. `ReqwestTransport` is the
default HTTP implementation; other HTTP clients and future RFC-backed
transports can implement the `Transport` trait. Stateless operations execute
directly through `Client`; operations requiring a persistent SAP user session
are represented separately as stateful operations.

Importing `TransportExt` adds `.traced()` to every concrete transport. The
decorator emits redacted structured `tracing` events that a CLI, language
server, or test subscriber can route to its preferred output.

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

A `UserSession` owns a cheap clone of its client. The transport and any loaded
capabilities remain shared through `Arc`, so the session has no borrowing
lifetime and can be stored for an entire editing workflow. Requests through one
session are serialized and carry its latest `sap-contextid`; separate user
sessions retain independent context IDs while sharing the client's HTTP security
session.
Call `UserSession::close()` when the workflow finishes. Dropping an instance
only releases local state and does not notify SAP.

### HTTP security-session resource

SAP also exposes a fixed HTTP security-session resource that was not advertised
by either discovery document on the tested A4H system:

```text
GET /sap/bc/adt/core/http/sessions
Accept: application/vnd.sap.adt.core.http.session.v3+xml
x-sap-security-session: create
sap-adt-purpose: logon
sap-adt-saplb: fetch
sap-cancel-on-close: true
```

Its response contains the current security-context reference, the configured
inactivity timeout, a current-session logoff link, and a system-information
link. The security-session relation targets:

```text
/sap/bc/adt/core/http/sessions/{security_context_reference}
```

The child resource only implements `DELETE`, but deleting it through the same
security session is intentionally a no-op. `CL_ADT_RES_HTTP_SESSION->DELETE`
compares the URI reference with the current request's security context and only
calls `CL_HTTP_SECURITY_SESSION_ADMIN=>TERMINATE_OLD_OWN_SESSION` when they
differ. It is therefore an old-session cleanup mechanism invoked from a newer
security session. The separately advertised `/sap/public/bc/icf/logoff`
resource logs off the current security session.

The server-side handlers observed on A4H are:

| Role | Handler |
| --- | --- |
| Registration | `CL_ADT_RES_HTTP_SESSION_APP->REGISTER_RESOURCES` |
| Collection GET | `CL_ADT_RES_HTTP_SESSION_COLL->GET` |
| Old-session DELETE | `CL_ADT_RES_HTTP_SESSION->DELETE` |

This resource manages the HTTP security session represented by
`SAP_SESSIONID_*`; it is unrelated to `UserSession::close()` and the
`sap-contextid` ABAP user session.

## Resource references

Resource references separate validated ADT locations from the operations that
act on them. A bare `ObjectRef` does not imply source, locking, update, or
execution capabilities:

| Type | Represents | Created from |
| --- | --- | --- |
| `ObjectRef` | A type-erased repository-object identity and location, without implied capabilities | An erased typed reference or a parsed ADT representation |
| `ObjectRef<T>` | An object identity tagged with a static `ObjectType` marker | `Client<Ready>::object::<T>(name)` |
| `SourceRef` | One source resource plus its owning object | An advertised source link or a source-capable reference such as `ObjectRef<Program>` |
| `ObjectRef<Program>` | A typed ABAP program reference | `Client<Ready>::object::<Program>(name)` |
| `ObjectRef<Include>` | A typed standalone-include reference | `Client<Ready>::object::<Include>(name)` |
| `ObjectRef<Package>` | A typed ABAP package reference | An embedded package reference or `Client<Ready>::object::<Package>(name)` |
| `OwnedResourceRef<T>` | Shared owner and link metadata for a typed relation reference | Relation resolution |
| `TextElementsRef`, `ObjectStructureRef`, and other relation references | Typed related resources plus their owning object | Fetched properties such as `ProgramProperties` |
| `AdtLink` | A resolved Atom link retaining its relation, representation metadata, query, fragment, and SAP ETag | A fetched resource representation |

Object-type markers provide their statically known category, allowing a
ready client to construct typed references without type-specific methods:

```rust,ignore
let program = client.object::<Program>("ZDEMO")?;
```

Domain references expose only the conventions established for that resource
type. Keeping the owning `ObjectRef` inside `SourceRef` lets the update builder
reject a `LockHandle` obtained for a different object before any request is sent.

## Program properties

`ObjectRef<Program>::query()` defaults to V3 before V2. Callers can replace that order;
the first preferred version advertised by central discovery is requested. V2
and V3 use the same payload schema, exposed as `ProgramPropertiesV2` and
`ProgramPropertiesV3` respectively (`ProgramPropertiesV2` is a type alias for
`ProgramPropertiesV3`). The payload is wrapped in the corresponding
`ProgramProperties::V2` or `ProgramProperties::V3` variant:

```rust,ignore
use zadt::{
    ObjectVersion, Operation, Program, ProgramProperties, ProgramPropertiesVersion,
};

let reference = client.object::<Program>("ZDEMO")?;
let response = reference
    .query()
    .priority([ProgramPropertiesVersion::V2, ProgramPropertiesVersion::V3])
    .version(ObjectVersion::WorkingArea)
    .execute(&client)
    .await?;
println!("media version: {:?}", response.media_version());

let properties = match response {
    ProgramProperties::V2(properties) | ProgramProperties::V3(properties) => properties,
    _ => panic!("unsupported program-properties version"),
};
let source = properties.source.query().execute(&client).await?;

assert_eq!(properties.package.name, "$TMP");
assert_eq!(properties.syntax_configuration.language.version, "X");
println!("text elements: {:?}", properties.text_elements()?);
println!("{}", source.content);
```

An unconditional query returns `ProgramProperties` directly. Calling
`.if_none_match(cached_etag)` changes the query mode and its response type to
`Conditional<ProgramProperties>`. A current ETag produces
`Conditional::NotModified { etag }`; a changed descriptor produces
`Conditional::Modified(representation)`. An unsolicited `304 Not Modified` is
rejected when no validator was supplied. HTTP response ETags are stored as
validated `EntityTag` values, so passing a fetched ETag to `.if_none_match()`
cannot fail during request construction. External strings can be validated with
`value.parse::<EntityTag>()`.

The optional version is typed as `ObjectVersion` and serializes to `active`,
`inactive`, `workingArea`, `new`, or `partlyActive`. These values come directly
from `IF_ADT_URI_QUERY_PARAMETERS`. `CL_SEDI_ADT_RES_SOURCE->GET` reads the
parameter and `CL_ADT_UTILITY->GET_WB_VERSION` maps it to the Workbench's
one-character `R3STATE`. Transient requests such as `WorkingArea` can therefore
produce a returned `ProgramPropertiesV3::version` of `Active` or `Inactive`.

The private Atom parser retains advertised links without resolving every target
up front. `ProgramPropertiesV3::relations()` and the nested
`SyntaxLanguage::relations()` preserve unknown relations alongside `rel`, media
type, title, language, length, query, fragment, and SAP ETag metadata. Their
iterators resolve links on demand, while typed accessors produce `HtmlSourceRef`,
`SourceVersionsRef`, `ObjectStructureRef`, `TextElementsRef`, enhancement
references, `ObjectStateRef`, and `ParserRef`. The required plain-text
`SourceRef` remains eagerly validated as part of the properties representation.
Bare relative, explicit `./`, root-relative, and query-bearing hrefs are resolved
against the fetched program while their paths remain validated beneath
`/sap/bc`.
`ObjectRef<Program>::source()` remains the direct conventional `source/main` reference;
`ProgramPropertiesV3::source` is the location advertised by SAP.

This conversion was verified against `Z_TEST` on the active A4H backend. V2
and V3 returned byte-identical XML bodies for that program and distinct,
correct response media types; a live `ProgramPropertiesQuery` successfully
converted all relations listed above.

## Program execution

`ObjectRef<Program>::run()` executes a program through the `programrun` URI template
advertised by central discovery and returns its rendered plain-text output:

```rust,ignore
use zadt::{Operation, Program};

let program = client.object::<Program>("ZDEMO")?;
let output = program.run().build()?.execute(&client).await?;
println!("{}", output.content);
```

Program names are canonicalized to uppercase when their references are created.
The operation is stateless, although its `POST` request still causes the HTTP
transport to acquire a CSRF token. An optional profiler trace can be attached
with `.profiler_id(id)` when the selected template advertises that variable.
Selection-screen parameters are not supported by this endpoint.

## Include properties

Standalone ABAP includes share the programs discovery scheme but use their own
collection and V2 representation. Resolution and media negotiation follow the
same split as programs:

```rust,ignore
use zadt::{Include, IncludeProperties, ObjectVersion, Operation};

let reference = client.object::<Include>("ZINCLUDE")?;
let response = reference
    .query()
    .version(ObjectVersion::Active)
    .execute(&client)
    .await?;
let properties = match response {
    IncludeProperties::V2(properties) => properties,
    _ => panic!("unsupported include-properties version"),
};
let source = properties.source.query().execute(&client).await?;
println!("{}", source.content);
```

`IncludePropertiesV2` retains its optional using-object context, package, source
relations, enhancement relations, and properties ETag. The implementation was
verified against `ZTEST` on the active A4H backend. `IncludePropertiesQuery`
supports the same `.if_none_match(etag)` transition and `Conditional` response
as program properties. Both public query names specialize the generic
`ObjectPropertiesQuery`, which centralizes discovery lookup, media negotiation,
object-version parameters, cache headers, and `200`/`304` handling.

## Object editing

Object locking and source updates are generic stateful operations. A
`ObjectRef<Program>` resolves its object and source resources from central discovery:

```rust,ignore
use zadt::{AccessMode, Operation, Program};

let session = client.create_user_session();
let program = client.object::<Program>("ZDEMO")?;
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

`ObjectRef<Program>::lock()` constructs a `LockRequest`, while
`SourceRef::update()` seeds an `ObjectSourceUpdateBuilder` with its validated
source. `ObjectLock` parses SAP's opaque `LOCK_HANDLE` into a `LockHandle`; the
update builder rejects a handle obtained for another object. Future domain
references can expose the same protocol operations when their ADT resource
profiles establish the corresponding capabilities.

The `UserSession` serializes both calls and carries the `sap-contextid` returned
by the lock response into the update request.

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
