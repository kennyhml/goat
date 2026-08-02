# zaff

Bidirectional ABAP File Formats projection for the Ziege tooling framework.

`zaff` maps repository objects from `zadt`/`zvfs` to editor-facing files and
maps those files back to their logical ADT components. It intentionally does
not fetch source or own a repository tree.

The initial scope covers the SAP ABAP File Formats layouts for programs,
standalone includes, and classes:

```rust
use zaff::{FileComponent, SourceComponent, resolve_file_name};
use zadt::ClassSourceComponent;

let file = resolve_file_name("zcl_example.clas.testclasses.abap")?;
assert_eq!(file.object_name, "ZCL_EXAMPLE");
assert_eq!(
    file.component,
    FileComponent::Source(SourceComponent::Class(
        ClassSourceComponent::TestClasses,
    )),
);
# Ok::<(), zaff::ProjectionError>(())
```

Path resolution identifies an AFF family and component, not a globally unique
remote object. A language server should retain the `RepositoryObjectEntry` used
to project every concrete path. That index disambiguates representations such
as `PROG/P` and `PROG/I`, which share the same `.prog.*` file layout, and keeps
the SAP system, package, URI, and object version attached to edits.

When no existing repository binding is available,
`ObjectFormat::repository_type_from_metadata` uses `programType` to distinguish
`PROG/I` from `PROG/P`. Missing `programType` follows AFF's
`executableProgram` default and therefore resolves to `PROG/P`.

Optional class includes and language-dependent property files are exposed as
possible `FileSpec`s. The projection consumer decides which concrete files to
publish based on resources available from the backend.

For a projected class path, the language server can recover the exact source
resource without treating the include as an independent object. The binding
check prevents a stale or colliding path from selecting another RIS entry:

```rust,ignore
let resolved = zaff::resolve_path(path)?;
let source = resolved.source_ref(repository_entry)?;
let source_code = source.query().execute(client).await?;
```

`source_ref` handles programs, standalone includes, and class source
components. Metadata and language-dependent properties files remain codecs for
the projection consumer rather than plain-text ADT source resources.
