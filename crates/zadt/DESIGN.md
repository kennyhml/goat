# ADT Design Notes

## Generic Properties Queries

Program and include properties use the same generic request and response kernel
while retaining domain-specific public names.

Current shape:

```rust
pub struct ObjectPropertiesQuery<T, M = Unconditional>
where
    T: ObjectProperties,
{
    resource: ObjectRef<T>,
    priority: Vec<T::MediaVersion>,
    version: Option<ObjectVersion>,
    mode: M,
}

pub type ProgramPropertiesQuery<M = Unconditional> =
    ObjectPropertiesQuery<Program, M>;
pub type IncludePropertiesQuery<M = Unconditional> =
    ObjectPropertiesQuery<Include, M>;
pub type ClassPropertiesQuery<M = Unconditional> =
    ObjectPropertiesQuery<Class, M>;
```

The generic layer should own:

- Discovery collection lookup by category.
- Ordered media-type negotiation and the exact `Accept` header.
- The optional `ObjectVersion` query parameter.
- `Cache-Control: no-cache` versus `If-None-Match`.
- `QueryMode`, `Conditional<T>`, and `200`/`304` handling.
- HTTP ETag extraction.

The protocol metadata is split across focused sealed profiles:

- `ObjectType` provides Workbench identity and naming constraints.
- `ObjectCollection` provides the canonical discovery `CategoryId`.
- `ObjectProperties` provides media-version and properties types plus the
  representation parser.

The generic query handles missing or unsupported response `Content-Type` values,
then passes the typed resource, negotiated media version, body, and ETag to the
profile parser. Properties types parse their own representations and return the
fixed `ResponseError` operation boundary while retaining domain-specific errors
as detailed causes.

Keep `ProgramProperties`, `IncludeProperties`, and future
`ClassProperties` separate. Their XML schemas, links, and domain semantics are
not interchangeable.

The sealed object-properties profile has this shape:

```rust
pub trait ObjectProperties: ObjectCollection {
    type MediaVersion: NegotiableMediaVersion;
    type Properties: Send;

    fn parse(
        resource: &ObjectRef<Self>,
        version: Self::MediaVersion,
        body: Vec<u8>,
        etag: Option<EntityTag>,
    ) -> Result<Self::Properties, ResponseError>;
}
```

## Class Properties Contract

Eclipse ADT 3.52 confirms that class properties fit this generic envelope:

```text
Discovery scheme: http://www.sap.com/adt/categories/oo
Discovery term:   classes
Collection:       /sap/bc/adt/oo/classes
Object URI:       /sap/bc/adt/oo/classes/{encoded-lowercase-classname}
Method:           GET
Optional query:   version=<object-version>
Conditional:      If-None-Match / 304 Not Modified
```

Class media priority:

```text
application/vnd.sap.adt.oo.classes.v4+xml
application/vnd.sap.adt.oo.classes.v3+xml
application/vnd.sap.adt.oo.classes.v2+xml
application/vnd.sap.adt.oo.classes+xml
```

Class-specific types should include `Class`, `ClassMediaVersion`,
`ClassProperties`, `ClassPropertiesRepresentation`, and `ClassError`.

Class source and class execution are separate contracts:

- Main source uses `/oo/classes/{class}/source/main` and `text/plain`.
- Additional class includes use `/oo/classes/{class}/includes/{include-type}`.
- Class execution uses the `oo` / `classrun` discovery template and `POST`.

Start with strict response `Content-Type` validation, consistent with program
and include properties. Only add a fallback to the requested representation if
a real backend omits the class-properties response header.
