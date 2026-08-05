# zadt

ABAP Development Tools protocol for the Ziege framework.

## Background

The ABAP Development Tools are more than just an Eclipse plugin. On the actual ABAP 
Application Server, they represent a whole framework for ABAP development tooling 
exposed via RFC or ICF (HTTP). The idea is similar to how the more modern language server
protocol works. Although while a language server intentionally decouples sender from receiver, 
ABAP development generally happens on the system itself and **must** serve language server like
features via external communication.

For anyone curious about them, you can enable the `ABAP Communication Log` view inside ecplise.

## Design

### Discovery

`ZADT` attempts to make proper use of the HATEOAS design of the ADT API. Unfortunately, this aspect is
almost always ignored by clients using such APIs, which makes it difficult for the provider of the API
to make changes that would otherwise be non breaking, such as changing the location of a resource.

Nevertheless, `ZADT` still uses if on the off-chance it ever happens, which also improves compatibility
going forward (and backwards). In simpler terms, this means when you create an object reference such as
```rust,ignore
let program = client.object::<Program>("ZDEMO")?;
```
The client checks the discovery collection for the stable `Program` schema and from there, determines 
that its location is advertised as `/sap/bc/adt/programs/programs`. Another advantage is that supported
media-types can be negotiated and chosen dynamically while remaining fully typed.

Its worth noting that HATEOAS doesnt mean the client does not know about the semantics of resources.
Operations and responsens remain fully typed. It mainly lets the provider change locations and 
syntactic details of a resource, such as the location (not the name!) of a uri parameter for a 
template action, without breaking clients relying on it.

### Typing

`ZADT` tries to provide full typing support and uses typestate pattern in various places to introduce
invariants. For example, the current typing system makes it impossible to call a stateful operation
without a stateful session context. It also provides convenient methods to navigate between objects
and their relations. You can find more on the technical details down below.

However, the reality is that the nature of the ADT client rarely makes typing meaningful. More often
than not, the kind of object youre working with comes from a command line or network request, at which
point it becomes difficult to transiton back to a fully typed object in all contexts. You might be thinking
that rust enums can come to the rescue here and I would agree, if ABAP did not have over one hundred
repository objects to cover. For this reason, the library makes an effort to provide type erasure via a
generic `RepositoryObject` trait that forward implements common operations such as locking or reading
the object properties. Operations such as reading the source code then end up being runtime checks
rather than failing at compile time. The downside is that operations that **return** object dependent
types, such as `ProgramProperties` vs `ClassProperties` can not reflect their concrete type at all.

### Transport

The transport layer can be conveniently abstracted away as, regardless of the protocol used, ADT wraps
the request and response in a HTTP-like structure anyways. RFC only ends up being a tunnel to transport
that data to and from the server. For this reason, `ZADT` supports custom, protocol agnostic transport
implementations. Currently only, HTTP transport using `reqwest` is implemented out of the box.


### Techical details

This is more of a flex about how awesome rust is and, unless you plan to contribute, can probably be
ignored.

The most important trait in `ZADT` is the `Operation` trait, defines as

```rust,ignore
pub trait Operation<S: ClientState>: Send + Sync {
    type Response: Send;
    type Kind: OperationKind;

    fn request(&self, client: &Client<S>) -> Result<AdtRequest, OperationError>;

    fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError>;
}
```

An operation is effectively a request which its implementor can construct in the `request` method.
It is generic over the client state `S`, which defines whether the client has performed discovery
and can provide operations with a collection, and the operation kind `OperationKind` which defines
whether an operation is `Stateful` or `Statelss`.

With that in place, an execution trait can be defined as
```rust,ignore
pub trait Execute<S, O>: Send + Sync
where
    S: ClientState,
    O: Operation<S>,
{
    fn execute(&self, operation: &O) -> impl Future<Output = Result<O::Response, OperationError>> + Send;
}
```
which means that a `Client` with state `S` can execute operations for state `S`.. This provides a compile 
time invariant for operations that rely on the discovery data to dispatch. Of course, entry point operations 
such as the discovery itself, are valid for any client state.

The associated type `Kind` enforces the next invariant, which is calling stateful operations outside
of a stateful context. To implement that, a user session wraps a client
```rust,ignore
pub struct UserSession<S: ClientState> {
    client: Client<S>,
    state: Mutex<UserSessionState>,
}
```
And then we implement the stateful execution only for `UserSession<S>`:
```rust,ignore
impl<S, O> Executo<S, O> for UserSession<S>
where
    S: ClientState,
    O: Operation<S, Kind = Stateful>,
{
    async fn execute(&self, operation: &O) -> Result<O::Response, OperationError> {
        ...
    }
}
```
The best part about this design is that it allows for some sweet decorator patterns. For instance,
etag handling can simply be implemented like this:
```rust,ignore
pub enum Revalidation<T> {
    Modified(T),
    NotModified { etag: Option<EntityTag> },
}

pub struct IfNoneMatch<O> {
    inner: O,
    etag: EntityTag,
}

impl<S, O> Operation<S> for IfNoneMatch<O>
where
    S: ClientState,
    O: Operation<S>,
{
    type Response = Revalidation<O::Response>;
    type Kind = O::Kind;

    fn request(&self, client: &Client<S>) -> Result<AdtRequest, OperationError> {
        let mut request = self.inner.request(client)?;
        request.set_cache_revalidation(Some(&self.etag));
        Ok(request)
    }

    fn decode(&self, response: OperationResponse) -> Result<Self::Response, ResponseError> {
        if response.status() == StatusCode::NOT_MODIFIED {
            return Ok(Revalidation::NotModified {
                etag: response.entity_tag(),
            });
        }

        self.inner.decode(response).map(Revalidation::Modified)
    }
}
```
This can also drive batching, paging, retry behavior and much more.
