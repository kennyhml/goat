# Eclipse ADT 3.52 Architecture Audit

This is a local research note. It is excluded through `.git/info/exclude` and
must not be committed.

Audit source:

```text
/mnt/e/eclipse/plugins
```

Methods used:

- `plugin.xml`, manifests, schemas, help, and localization resources.
- JAR archive inspection.
- JVM bytecode inspection and decompilation.
- Targeted searches for protocol constants, classes, and call sites.

Limits:

- No live packet capture was performed.
- Native `SapGuiServer.exe` internals were not available.
- Installed SAP GUI for Java implementation internals were not audited.
- Backend implementations of `SADT_START_TCODE` and `SADT_START_WB_URI` were
  not available.
- Optional Eclipse extensions not installed in this environment are outside
  this report.

## Architecture Overview

ADT separates these concerns:

| Layer | Responsibility |
| --- | --- |
| Destination model | Finds systems and persists connection metadata |
| Communication | Authentication, sessions, cookies, CSRF, HTTP/RFC dispatch |
| Compatibility | Core/central discovery, templates, compatibility graph |
| Project | Associates an Eclipse project with a destination |
| SAP GUI integration | Starts native GUI sessions and navigates objects |
| Semantic filesystem | Lazily caches properties and source |

Important conclusions:

- SSO is not one mechanism. HTTP, cloud, classic RFC, and SAP GUI use different
  authentication paths.
- Classic systems can tunnel HTTP-shaped ADT requests through RFC function
  `SADT_REST_RFC_ENDPOINT`.
- Direct HTTP ADT performs an explicit `/core/http/sessions` bootstrap.
- SAP GUI is embedded natively. ADT does not use `sapshcut`, WebGUI, SAP GUI
  scripting, or attach to existing SAP Logon sessions.
- Source bodies are lazy semantic-filesystem blobs. Properties act as a
  manifest containing source links and ETags.
- A warm source cache hit performs no backend request.
- A save caches server-confirmed content rather than blindly retaining the
  submitted editor bytes.

## Authentication And Single Sign-On

### Installed Bundles

```text
/mnt/e/eclipse/plugins/com.sap.adt.destinations.model_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.communication_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.util_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.tools.cloud.authentication.ui_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.project_3.52.0.jar
/mnt/e/eclipse/plugins/org.eclipse.core.net_1.5.700.v20250313-0656.jar
```

### Authentication Matrix

| Mechanism | Scope |
| --- | --- |
| Preemptive HTTP Basic | Direct ADT HTTP destinations |
| TLS client certificate | Direct HTTPS destinations with SSO enabled |
| Browser reentrance ticket | ABAP Cloud project login |
| SAP GUI reentrance ticket | Embedded Java/Windows SAP GUI |
| SNC | Classic JCo and SAP GUI connectivity |
| Username/password | Classic JCo and SAP GUI fallback |
| Proxy Basic/Digest | Eclipse HTTP/HTTPS proxy |
| SPNEGO/Kerberos target HTTP | No installed provider found |
| OAuth bearer target HTTP | No implementation found |

### HTTP Basic

Relevant SPI and implementation:

```text
com.sap.adt.destinations.model.http.authentication.IHttpAuthenticationHandler
com.sap.adt.destinations.model.http.internal.authentication.basicauth.HttpBasicAuthHandler
```

Wire form:

```http
Authorization: Basic Base64(US-ASCII(user:password))
```

Behavior:

- Authentication is preemptive.
- Apache target challenge negotiation uses a null authentication strategy.
- An alias user takes precedence when configured.
- Passwords are volatile destination properties.
- Destination serializers exclude volatile authentication data.
- US-ASCII encoding can mishandle non-ASCII credentials.

Serialization evidence:

```text
com.sap.adt.destinations.model.http.internal.HttpDestinationDataSerializer
  serializeToByteArray
  serializeDomainSpecificPersistentPropertiesToByteArray
```

### TLS Client-Certificate SSO

Relevant code:

```text
com.sap.adt.communication.http.internal.apacheclient.ApacheCommonsHttpConnection
  createHttpClient

com.sap.adt.util.keystore.KeyStoreUtil
  getSSLContext
```

Supported key/trust sources include:

```text
Windows-ROOT
Windows-MY
JVM default keystores
javax.net.ssl.keyStore
javax.net.ssl.keyStoreType
javax.net.ssl.keyStoreProvider
javax.net.ssl.keyStorePassword
PKCS11 with keyStore=NONE
```

`CollectionKeyManager.chooseClientAlias` selects the first suitable alias.
There is no observed ADT-specific certificate selection dialog.

If server-certificate validation is disabled,
`CollectionTrustManager.checkServerTrusted` logs and ignores trust failures.
That removes effective server identity verification.

### Cloud Browser Login

Flow:

```text
Start local callback server
-> open backend reentrance-ticket URL in browser
-> receive ticket at localhost callback
-> send one-shot MYSAPSSO2 authentication header
-> perform preflight validation
-> keep resulting cookies in volatile state
-> stop callback server
```

Endpoints:

```text
/sap/bc/adt/core/http/reentranceticket
  ?redirect-url=http://localhost:<port>/adt/redirect

/adt/redirect?reentrance-ticket=<ticket>
```

Classes:

```text
com.sap.adt.tools.cloud.authentication.ui.internal.httpserver.AdtLocalHttpServer
com.sap.adt.tools.cloud.authentication.ui.internal.reentrance.AdtReentranceTicketResourceUtil
com.sap.adt.tools.cloud.authentication.ui.internal.reentrance.SamlWithReentranceTicketAuthenticationHandler
com.sap.adt.tools.cloud.authentication.ui.internal.reentrance.dialog.SamlWithReentranceTicketProjectLogonDialog
```

Cloud service-key handling validates OAuth-shaped JSON but only extracts the
ABAP service URL. No UAA token exchange or service-key credential persistence
was found.

### HTTP Security-Session Bootstrap

Direct HTTP destinations use:

```http
GET /sap/bc/adt/core/http/sessions
x-sap-security-session: create
sap-adt-purpose: preflight_logon
sap-client: <client>
sap-language: <language>
Accept: application/vnd.sap.adt.core.http.session.v3+xml
```

Accepted response versions:

```text
application/vnd.sap.adt.core.http.session.v3+xml
application/vnd.sap.adt.core.http.session.v2+xml
application/vnd.sap.adt.core.http.session.v1+xml
```

The response can advertise:

- Logoff URI.
- Security-session logoff URI.
- Old-session cleanup URI.
- Inactivity timeout.
- System-information relation.

System information uses:

```text
application/vnd.sap.adt.core.http.systeminformation.v1+json
```

Relevant classes:

```text
com.sap.adt.communication.http.systemconnection.IHttpSystemConnection
com.sap.adt.communication.http.internal.systemconnection.HttpSystemConnection
com.sap.adt.communication.http.internal.httpconnection.PreflightAccessDataProvider
com.sap.adt.communication.http.internal.httpconnection.HttpSessionInformationContentHandler
com.sap.adt.communication.http.internal.dispatcher.HttpRequestDispatcher
```

Benefits of this bootstrap:

- Deterministic credential/client/language/system validation.
- Explicit HTTP security-session cookie establishment.
- Load-balancer affinity.
- Backend-provided lifecycle and cleanup links.
- Inactivity handling.
- Reliable relogon after session loss.
- A stable context for CSRF token handling.

This is not the stateful ABAP user session represented by `sap-contextid`.

```text
HTTP security session:
  authentication, cookies, affinity, CSRF, logoff

ADT user session:
  locks, stateful ABAP context, sap-contextid
```

### Retry And Session Recovery

Dispatcher modes include:

```text
READ
READ_RETRY
WRITE
WRITE_RETRY
CSRF_TOKEN_FETCH
CSRF_TOKEN_FETCH_RETRY
LOGON
RELOGON
```

Behavior:

- `401`: reset session/cookies, relogon once, retry eligible requests.
- `403` plus `x-csrf-token: Required`: refresh CSRF and retry once.
- `400 ICMENOSESSION`: retry eligible soft-state/enqueue requests once.
- Hard-state session loss closes the session and fails.
- Retry variants prohibit another recursive retry.

Relevant classes:

```text
com.sap.adt.communication.http.internal.dispatcher.HttpRequestDispatcher
com.sap.adt.communication.http.internal.dispatcher.HttpDispatcherMode
```

### Logoff And Cleanup

`HttpSystemConnection` stores the backend cleanup URI in destination instance
preferences. A later process can delete a stale old session. Normal logoff uses
the advertised logoff URI, resets cookies, and disposes the connection.

Relevant methods:

```text
HttpSystemConnection.dispose
HttpSystemConnection.resetLoggedOnState
HttpSystemConnection.sendRequestForLogoffViaStatelessSession
HttpSystemConnection.updateCleanUpUriInPreferences
HttpSystemConnection.deleteSessionOnServer
HttpLowLevelLogoffResource.logoff
```

### Credential Persistence

Project/destination serializers persist users and connection metadata but not:

- Passwords.
- Reentrance tickets.
- Authentication cookies.

Classic passwords remain in process memory through authentication tokens and
the JCo destination registry until the project or ADT closes.

Proxy credentials are different. Eclipse stores them in Equinox secure
preferences beneath:

```text
/org.eclipse.core.net.proxy.auth/<HTTP|HTTPS|SOCKS>
```

Relevant classes:

```text
com.sap.adt.communication.http.internal.apacheclient.EclipseProxyPreferencesCredentialsProvider
org.eclipse.core.internal.net.ProxyType
```

### Authentication Security Findings

- Java GUI INFO tracing can include raw `sso2` tickets.
- Windows GUI command tracing masks `password` but not ticket-bearing `cookie`.
- `SapGuiStartupData.toString()` includes the reentrance ticket.
- Disabling TLS certificate checking removes endpoint authentication.
- Basic authentication uses US-ASCII.

## System Landscape And Discovery

### Finding Systems

ADT reads SAP UI Landscape XML, not legacy `SAPLOGON.ini`.

Relevant bundle and classes:

```text
/mnt/e/eclipse/plugins/com.sap.adt.destinations.model_3.52.0.jar

com.sap.adt.destinations.model.internal.config.SapUiLandscapeReader
com.sap.adt.destinations.model.internal.config.SAPGUIConfigWrapper
com.sap.adt.destinations.model.internal.config.SystemConfigurationService
com.sap.adt.destinations.model.internal.preferences.DestinationModelPreferences
```

Imported data includes:

- Application server and instance.
- Message server and logon group.
- Message-server service.
- SAProuter.
- Gateway.
- SNC partner and quality.
- SSO flags.
- Preferred client, language, and user.

Windows resolution includes:

```text
last-used landscape registry values
SAPLOGON_LSXML_FILE
SAP Logon registry configuration
SAPUILandscape.xml
SAPUILandscapeGlobal.xml
```

SAP GUI for Java conventionally uses `SAPGUILandscape.xml`, with Linux data
beneath `${user.home}/.SAPGUI/`.

Override preferences live under `com.sap.adt.destinations.model`:

```text
overrideXmlLocations
xmlLocalPath
xmlGlobalPath
```

### Project Persistence

Connection files:

```text
.destination.properties
.destination.http.properties
.destination.http.domain.properties
```

Relevant class:

```text
com.sap.adt.project.internal.AdtCoreProject
```

Classic fields include server/message-server settings, router, gateway, SNC,
SSO, user, client, language, and linked SAP Logon configuration name.

HTTP fields include service URL, system ID, authentication kind, user, client,
language, and explicitly persistent authentication properties.

Passwords, tickets, and cookies are excluded.

### Classic RFC Bridge

Classic destinations are converted to `jco.client.*` properties and registered
in memory:

```text
com.sap.adt.communication.internal.jco.JCoDestinationRegistry
```

HTTP-shaped ADT requests are bridged through:

```text
SADT_REST_RFC_ENDPOINT
```

Relevant implementation:

```text
com.sap.adt.communication.internal.jco.dispatcher.JCoRequestDispatcherRestProtocolStrategy
```

This validates a transport-neutral ADT request model: typed operations need not
know whether they travel over direct HTTP or RFC.

### Direct HTTP And Cloud Bootstrap

Approximate sequence:

```text
service URL
-> /sap/public/bc/icf/virtualhost
-> Basic or browser reentrance authentication
-> /sap/bc/adt/core/http/sessions
-> system information
-> core discovery
-> compatibility graph
-> central discovery
```

The ADT direct HTTP destination model requires HTTPS.

### Core And Central Discovery

Endpoints:

```text
/sap/bc/adt/core/discovery
/sap/bc/adt/discovery
```

Media type:

```text
application/atomsvc+xml
```

Core discovery provides infrastructure capabilities. Central discovery provides
domain collections such as programs, includes, classes, and runtime services.

Relevant bundle and classes:

```text
/mnt/e/eclipse/plugins/com.sap.adt.compatibility_3.52.0.jar

com.sap.adt.compatibility.discovery.AdtDiscoveryFactory
com.sap.adt.compatibility.internal.discovery.DiscoveryCache
com.sap.adt.compatibility.internal.discovery.DiscoveryProxy
com.sap.adt.compatibility.internal.discovery.DiscoveryContentHandler
```

`DiscoveryCache` is process-local and keyed by destination plus discovery URI.

Behavior:

- Reentrant lookup returns temporary empty discovery.
- Forbidden discovery can be cached as empty with warning status.
- Resource/logon/system failures can become cached empty error discovery.
- Cancellation does not install a delegate.
- Malformed individual collections are logged and skipped.
- Destination removal invalidates discovery and compatibility caches.

The URI-template extension is parsed by:

```text
com.sap.adt.compatibility.model.templatelink.util.AdtTemplateLinkXMLProcessor
```

Namespace and elements:

```text
http://www.sap.com/adt/compatibility
adtcomp:templateLinks
adtcomp:templateLink
```

### Compatibility Graph

Discovery identity:

```text
scheme: http://www.sap.com/adt/categories/compatibility
term: graph
```

Fallback URI:

```text
/sap/bc/adt/compatibility/graph
```

Relevant classes:

```text
com.sap.adt.compatibility.internal.graph.provider.GraphProviderFactory
com.sap.adt.compatibility.base.internal.graph.provider.GraphAnalyzer
```

The graph intersects client-declared feature nodes with backend nodes. Missing
obligatory features produce `OutDatedClientException`.

Statuses commonly interpreted as incompatibility include:

```text
405
410
415
most 406 responses
400 ExceptionParameterNotFound
```

## SAP GUI Integration

### Installed Bundles

```text
/mnt/e/eclipse/plugins/com.sap.adt.sapgui.ui_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.sapgui.ui.win32_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.sapgui.embedding_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.sapgui.config_3.52.0.jar
```

### What ADT Does Not Use

No installed evidence was found for:

```text
sapgui://
r3://
sapshcut.exe
WebGUI/ITS
SAP GUI scripting
SAP ROT attachment
```

ADT embeds native SAP GUI implementations instead.

### Launch Flow

```text
ADT command or object selection
-> ensure project is logged on
-> obtain a fresh SAP GUI reentrance ticket
-> build backend proxy transaction
-> start embedded Java GUI or Windows SapGuiServer
-> SAP GUI establishes its backend GUI session
-> navigation events return to Eclipse
```

### Proxy Transactions

Transaction launch:

```text
*SADT_START_TCODE
D_AIE_TCODE=<transaction>
D_ECLIPSE_NAVIGATION=<X-or-space>
D_GUID=<uuid>
D_ECLIPSE_PROJECT=<project>
```

Workbench object launch:

```text
*SADT_START_WB_URI
D_OBJECT_URI=<VIT-uri-part-1>
D_OBJECT_URI_EXT=<optional-part-2>
D_WB_ACTION=<DISPLAY|CREATE|DELETE|EXECUTE|...>
D_ECLIPSE_NAVIGATION=<X-or-space>
D_GUID=<uuid>
D_ECLIPSE_PROJECT=<project>
```

Optional fields include:

```text
D_PARAMETERS
D_REQUEST_USER
D_IDE_USER
D_IDE_ID
D_TID
D_TEST_MODE
D_TRACE_ID
```

### VIT

No authoritative expansion of the acronym `VIT` exists in the installed ADT
artifacts. Do not invent one.

For the Workbench branch, a VIT URI is a system-relative backend identity for a
classic repository object:

```text
/sap/bc/adt/vit/wb/object_type/<workbench-type-key>/object_name/<name>
```

Examples:

```text
/sap/bc/adt/vit/wb/object_type/devck/object_name/%24TMP
/sap/bc/adt/vit/wb/object_type/dtelde/object_name/WDY_BOOLEAN
/sap/bc/adt/vit/wb/object_type/trant/object_name/SE38
```

Common type normalization:

```text
DEVC/K  -> devck
DTEL/DE -> dtelde
DOMA/DD -> domadd
TRAN/T  -> trant
```

Backend type metadata and URI templates remain authoritative.

A repository result can contain both:

```text
OBJECT_URI      native ADT resource
OBJECT_VIT_URI  classic Workbench/SAP GUI fallback
```

`OBJECT_VIT_URI` is not an alternate spelling of `OBJECT_URI`. It is the
backend-provided classic Workbench identity used when native ADT support is
missing or a classic action is required.

Relevant classes:

```text
com.sap.adt.sapgui.ui.urimapping.SapGuiUriMappingHandler
com.sap.adt.ris.search.repositoryservice.RepositoryObjectListItem
com.sap.adt.tools.core.urimapping.AdtVitUriMappingService
```

The VIT-to-native mapper uses:

```text
category scheme: http://www.sap.com/adt/categories/vit/urimapper
category term: vitUriMapping
relation: http://www.sap.com/adt/vit/uriMapper
```

The generic VIT editor is read-only. It is used when SAP GUI is unavailable and
basic properties are supported. It requests:

```text
application/vnd.sap.adt.basic.object.properties+xml
```

### Java GUI

ADT loads the installed Java GUI into the Eclipse JVM using:

```text
GuiStartS.jar
com.sap.platin.micro.Microkernel
com.sap.platin.base.logon.GuiImpl
```

Connection parameters can include:

```text
tran
clnt
user
lang
sncon
sncqop
sncname
sso2
manualLogin
pass
wp
```

One embedded application is retained, while editor tabs receive separate GUI
connections.

### Windows GUI

Process invocation:

```text
SapGuiServer.exe \\.\pipe\AiEWinguiEventpipe-<uuid>
```

Communication uses UTF-16LE XML over named pipes. The pipe ACL is restricted to
the current Windows SID.

Each `CreateSession` command carries connection details, proxy transaction,
window handle, client/user/language, and either:

```text
cookie=<reentrance-ticket>
```

or password/manual-login fallback.

Native SAP connection reuse is explicitly disabled:

```text
reuseConnection=0
```

ADT can reuse an ADT-owned GUI editor by stopping its current transaction and
issuing `/n...`. It does not attach to an existing SAP Logon session.

### Reverse Navigation

Recognized events include:

```text
EDIT
VIEW
WB_NAVIGATE
WB_REQUEST_FINISHED
WB_FINISHED
WB_EXPLORER_REFRESH
INIT_SERVER_SESSION
```

Relevant fields include:

```text
RESOURCE_URL
ECLIPSE_PROJECT
SYSTEM_ID
USER_ID
WB_ACTION
EDITOR_GUID
SERVER_SESSION_URI
```

ADT opens returned resources through its normal navigation service.

### SAP GUI Security Findings

- `SAPGUI_PIPE_SERVER` can override the Windows executable path.
- Java GUI code is classloaded with Eclipse process privileges.
- The named pipe trusts the current user, not strictly the spawned child PID.
- Ticket-bearing Java startup strings can be logged at INFO.
- Windows XML tracing masks passwords but not ticket cookies.
- HTTP-only destinations cannot launch SAP GUI.
- SAP UI Landscape supplies connection configuration, not an existing login
  session or credential-store handoff.

## Local Properties And Source Cache

### Installed Bundles

```text
/mnt/e/eclipse/plugins/org.eclipse.core.resources.semantic_0.8.0.sap.jar
/mnt/e/eclipse/plugins/com.sap.adt.tools.filesystem_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.tools.abapsource_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.tools.abapsource.ui_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.programs_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.oo_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.activation_3.52.0.jar
/mnt/e/eclipse/plugins/com.sap.adt.activation.ui_3.52.0.jar
```

### Logical Paths

Projects use:

```text
semanticfs:/<project>
```

Names are path-segment encoded and lowercased.

Program files:

```text
.adt/programs/programs/<name>/<name>.approg
.adt/programs/programs/<name>/<name>.asprog
```

Class files:

```text
.adt/classlib/classes/<name>/<name>.apclass
.adt/classlib/classes/<name>/<name>.aclass
.adt/classlib/classes/<name>/<name>definitions.acinc
.adt/classlib/classes/<name>/<name>implementations.acinc
.adt/classlib/classes/<name>/<name>macros.acinc
.adt/classlib/classes/<name>/<name>testclasses.acinc
.adt/classlib/classes/<name>/<name>localtypes.acinc
```

Path services:

```text
AbapProgramFilePathService
AbapClassFilePathService
AbapFilePathService
UriUtils.encodePathSegment
```

### Physical Storage

Blob root:

```text
<workspace>/.metadata/.plugins/org.eclipse.core.resources.semantic/.cache/
```

Metadata:

```text
metadata.xmi
$.<project>.xmi
```

Relevant classes:

```text
org.eclipse.core.resources.semantic.spi.SemanticFileCache
org.eclipse.core.internal.resources.semantic.SemanticResourcesPlugin
org.eclipse.core.internal.resources.semantic.SemanticMetadataPersistenceManager
org.eclipse.core.internal.resources.semantic.cacheservice.CacheService
```

### Cache Hit And Miss

Relevant flow:

```text
CachingContentProvider.openInputStream
-> cache hit: return cached bytes, no backend GET
-> cache miss: AdtContentProvider.openInputStreamInternal
-> fetch backend content
-> populate cache
```

A logged-out miss can return and cache zero bytes. This can later resemble valid
empty source. A future Ziege cache must return an explicit offline-cache-miss
state instead.

### Conditional Synchronization

`If-None-Match` is sent only when both an ETag and cached bytes exist.

Relevant methods:

```text
AdtContentProvider.isConditionalGetFeasible
AdtContentProvider.getRequestHeadersForConditionalGet
AdtContentProvider.synchronizeFileWithRemote
```

HTTP 304 preserves the existing blob without rewriting it.

### Properties As Compound Manifest

Compound objects fetch properties first. Source links and ETags in those
properties determine cache invalidation.

Behavior:

- New sources are attached without eagerly downloading bodies.
- Changed source ETags delete cached source blobs.
- Unchanged source components retain cached bodies.
- Sources absent from new properties are removed.
- Bodies are fetched lazily on the next cache miss.

Relevant classes and methods:

```text
AdtContentProviderForCompoundObjects.internalSynchronizeContentWithRemote
AdtContentProviderForCompoundObjects.syncSourceFilesWithNewOrChangedFilesFromBackend
AdtContentProviderForCompoundObjects.syncSourceFilesWithDeletedFilesFromBackend
AbapSourceContentProviderForCompoundObjects.determineEtagForSourceFile
AbapProgramContentProvider.getSourceFileStores
AbapClassContentProvider.getSourceFileStores
```

### Save Transaction

Flow:

```text
editor output
-> temporary cache handle
-> validate append/content type/lock/update state
-> remote PUT
-> use PUT response or follow-up GET as canonical bytes
-> update ETags and metadata
-> roll back temporary submitted bytes
```

Relevant methods:

```text
CachingOutputStream.close
AdtContentProvider.beforeCacheUpdate
AdtContentProvider.getQueryParametersForUpdateRequest
AdtContentProvider.saveFileContentToRemote
```

Consequences:

- Backend-normalized content wins over submitted bytes.
- Remote failure preserves the prior canonical cache.
- Append mode is unsupported.

### Refresh And Conflict Handling

Manual refresh is refused while an editor/model buffer is dirty.

A clean forced refresh:

```text
clear sibling ETags
-> synchronize compound object
-> reload properties/model
-> reload loaded source pages
-> restore selection/folding
```

Relevant classes:

```text
AbapSourcePage.RefreshAdtSourcePageAction
AbapSourceMultiPageEditor
AdtSfsUtil
```

Source conflict choices:

```text
keep local
take backend
three-way merge
```

Relevant class:

```text
AbapMergeHelperSourceAndModelBased
```

It records ancestor source around lock/save, compares timestamps, opens the merge
dialog, and transfers the result back into the editor. If no ancestor is
available, it falls back to current client source.

Properties/model conflicts generally offer only take-yours or take-theirs.

### Local And Backend History

Eclipse local history uses:

```text
org.eclipse.core.internal.localstore.HistoryStore2
```

ADT backend history uses:

```text
IAdtObjectHistoryService
AdtSourceFileHistoryProvider
AdtSourceSemanticFileRevision
```

These are distinct facilities and should remain separate in Ziege.

### Activation

Successful activation schedules incoming semantic synchronization:

```text
ISemanticFile.synchronizeContentWithRemote(SyncDirection.BOTH, ...)
```

For these providers, `BOTH` follows the incoming synchronization path rather
than uploading dirty content.

Relevant classes:

```text
AdtActivationService
AbstractActivationHandler
```

## Class Properties Fit

Class properties use the same transport envelope as program/include properties:

```http
GET /sap/bc/adt/oo/classes/{classname}?version=...
Accept: application/vnd.sap.adt.oo.classes.v4+xml
If-None-Match: ...
```

Discovery:

```text
scheme: http://www.sap.com/adt/categories/oo
term: classes
collection: /sap/bc/adt/oo/classes
```

Media priority:

```text
application/vnd.sap.adt.oo.classes.v4+xml
application/vnd.sap.adt.oo.classes.v3+xml
application/vnd.sap.adt.oo.classes.v2+xml
application/vnd.sap.adt.oo.classes+xml
```

Class-specific models and parsers must remain separate. Class source and
`classrun` are separate contracts.

## Ziege Architecture Recommendations

### Explicit Initial Logon State

Recommended lifecycle:

```text
Client<Unauthenticated>
  -> logon()
Client<LoggedOn>
  -> discover()
Client<Discovered>
```

`Discovered` remains logged on. Normal operations should require a
`LoggedOnState` capability.

Initial logon should be explicit because it can:

- Fail credentials.
- Open a browser for SSO.
- Validate system/client/user/language.
- Establish lifecycle and cleanup links.
- Produce meaningful session metadata.

Automatic behavior after initial logon should include:

- Cookie reuse.
- Load-balancer affinity.
- CSRF acquisition and one retry.
- One automatic relogon after expired `401`.
- Inactivity/session bookkeeping.

Discovery need not be one-shot. `DiscoveryQuery` can work for every
`LoggedOnState`, with `rediscover()` or `refresh_capabilities()` replacing the
stored capabilities.

### Communication Priorities

1. Add an authentication strategy abstraction.
2. Implement `/core/http/sessions` bootstrap.
3. Parse and honor logoff, cleanup, inactivity, and system-information links.
4. Retry once on `401` relogon and `403 x-csrf-token: Required`.
5. Load core discovery and compatibility graph in addition to central discovery.
6. Decide explicitly between strict discovery parsing and ADT per-collection
   tolerance.
7. Keep direct HTTP, cloud browser auth, and classic RFC setup separate.

### Cache Requirements

1. Separate canonical server content from dirty drafts.
2. Represent metadata-known/content-absent, content-present, dirty, deleted, and
   invalid states explicitly.
3. Never represent an offline cache miss as empty source.
4. Key by destination identity, SAP client, object URI, object version, and
   source component.
5. Synchronize properties before source bodies.
6. Invalidate source bodies from advertised source ETags.
7. Commit bytes and metadata atomically.
8. Treat server save responses as canonical.
9. Persist a merge base for editable drafts.
10. Keep local snapshots separate from backend revision history.

### SAP GUI Boundary

SAP GUI integration should live outside the core ADT protocol crate. A platform
integration layer would own:

- SAP UI Landscape import.
- Reentrance-ticket acquisition.
- Native Java/Windows GUI startup.
- Proxy transaction construction.
- Reverse navigation.
- Credential and ticket redaction.

A smaller first step could expose typed VIT/SAP GUI launch descriptors without
attempting native embedding.

## Evidence Bundle Index

```text
com.sap.adt.destinations.model_3.52.0.jar
com.sap.adt.communication_3.52.0.jar
com.sap.adt.util_3.52.0.jar
com.sap.adt.project_3.52.0.jar
com.sap.adt.compatibility_3.52.0.jar
com.sap.adt.compatibility.base_3.52.0.jar
com.sap.adt.tools.cloud.authentication.ui_3.52.0.jar
com.sap.adt.sapgui.ui_3.52.0.jar
com.sap.adt.sapgui.ui.win32_3.52.0.jar
com.sap.adt.sapgui.embedding_3.52.0.jar
com.sap.adt.sapgui.config_3.52.0.jar
org.eclipse.core.resources.semantic_0.8.0.sap.jar
com.sap.adt.tools.filesystem_3.52.0.jar
com.sap.adt.tools.abapsource_3.52.0.jar
com.sap.adt.tools.abapsource.ui_3.52.0.jar
com.sap.adt.programs_3.52.0.jar
com.sap.adt.oo_3.52.0.jar
com.sap.adt.activation_3.52.0.jar
com.sap.adt.activation.ui_3.52.0.jar
org.eclipse.core.resources_3.22.200.v20250513-1234.jar
org.eclipse.core.net_1.5.700.v20250313-0656.jar
```
