# ADT Design Notes

## Generic Properties Queries

Program and include properties currently use the same request and response
algorithm. Before adding class properties, consolidate that algorithm into a
generic query kernel while preserving domain-specific public names.

Proposed shape:

```rust
pub struct ObjectPropertiesQuery<R, M = Unconditional>
where
    R: ObjectProperties,
{
    resource: R,
    priority: Vec<R::MediaVersion>,
    version: Option<ObjectVersion>,
    mode: M,
}

pub type ProgramPropertiesQuery<M = Unconditional> =
    ObjectPropertiesQuery<ProgramRef, M>;
pub type IncludePropertiesQuery<M = Unconditional> =
    ObjectPropertiesQuery<IncludeRef, M>;
pub type ClassPropertiesQuery<M = Unconditional> =
    ObjectPropertiesQuery<ClassRef, M>;
```

The generic layer should own:

- Discovery collection lookup by category.
- Ordered media-type negotiation and the exact `Accept` header.
- The optional `ObjectVersion` query parameter.
- `Cache-Control: no-cache` versus `If-None-Match`.
- `QueryMode`, `Conditional<T>`, and `200`/`304` handling.
- HTTP ETag extraction.

Each sealed resource profile should provide:

- Its typed reference and resource URI.
- Its discovery `CategoryId`.
- Its associated media-version and representation types.
- Its domain error conversion.

The version-tagged representation implements
`TryFrom<RawObjectProperties<Reference>>` in the models layer. That conversion
owns XML parsing and semantic validation. The generic query handles missing or
unsupported response `Content-Type` values before handing the owned body to the
model conversion.

Keep `ProgramProperties`, `IncludeProperties`, and future
`ClassProperties` separate. Their XML schemas, links, and domain semantics are
not interchangeable.

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

Class-specific types should include `ClassRef`, `ClassMediaVersion`,
`ClassProperties`, `ClassPropertiesRepresentation`, and `ClassError`.

Class source and class execution are separate contracts:

- Main source uses `/oo/classes/{class}/source/main` and `text/plain`.
- Additional class includes use `/oo/classes/{class}/includes/{include-type}`.
- Class execution uses the `oo` / `classrun` discovery template and `POST`.

Start with strict response `Content-Type` validation, consistent with program
and include properties. Only add a fallback to the requested representation if
a real backend omits the class-properties response header.
