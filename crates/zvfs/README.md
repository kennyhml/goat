# zvfs

A lazy virtual repository tree backed by the ADT Repository Information System.

## Background

SAP systems store repository objects in database tables rather than as local
text files. Opening an object therefore requires a connection to the SAP system
and an understanding of the remote repository protocol. The repository is not
simply a remote filesystem.

The ABAP Development Tools (ADT) expose the Repository Information System (RIS)
and its virtual-folders API for discovering repository objects. `zvfs` uses
[zadt](../zadt) for that communication and provides a higher-level API for
traversing and refreshing the resulting repository tree.

## Design

RIS structures repository objects through facets. A facet is a metadata
dimension such as package, owner, broad repository group, or concrete object
type. A request filters objects with facet preselections and names an output
facet by which RIS should group the matching objects.

For example, this request asks for packages containing classes owned by `DEVELOPER`:
```xml
<vfs:preselection facet="OWNER">
    <vfs:value>DEVELOPER</vfs:value>
</vfs:preselection>
<vfs:preselection facet="TYPE">
    <vfs:value>CLAS</vfs:value>
</vfs:preselection>
<vfs:facetorder>
    <vfs:facet>PACKAGE</vfs:facet>
</vfs:facetorder>
```

`zvfs` turns these requests and responses into caller-defined `Mount` points. A
mount is a static entry point into a facet chain that eventually leads to
repository objects. Eclipse with ADT commonly presents entries like:

```text
A4H
├── Local Objects ($TMP)
├── Favorite Packages
├── Favorite Objects
└── System Library
```

A local-objects mount can be constructed like this:

```rust,no_run
use zadt::{Client, Ready, RepositoryFacet, RepositoryPreselection};
use zvfs::{FacetLevel, FacetPolicy, Mount, VfsError, VirtualRepositoryTree};

async fn local_objects_tree(client: Client<Ready>) -> Result<VirtualRepositoryTree, VfsError> {
    let preselections = [
        RepositoryPreselection::directly_assigned("$TMP"),
        RepositoryPreselection::new(RepositoryFacet::OWNER, "DEVELOPER"),
    ];

    VirtualRepositoryTree::builder(client)
        .mount(
            Mount::selection("Local Objects ($TMP)", preselections).facet_policy(
                FacetPolicy::new([
                    FacetLevel::always(RepositoryFacet::OWNER),
                    FacetLevel::always(RepositoryFacet::GROUP),
                    FacetLevel::always(RepositoryFacet::TYPE),
                ]),
            ),
        )
        .build()
        .await
}
```

To include another owner's local objects, add that owner to the same preselection:

```rust
use zadt::{RepositoryFacet, RepositoryPreselection};

let preselections = [
    RepositoryPreselection::directly_assigned("$TMP"),
    RepositoryPreselection::new(RepositoryFacet::OWNER, "DEVELOPER").include("JONDOE"),
];
```
The resulting tree could look like:

```text
A4H
└── Local Objects ($TMP)
    ├── DEVELOPER
    │   ├── Dictionary
    │   │   └── Database Tables
    │   │       └── ZMYTAB
    │   └── BSP Library
    └── JONDOE
        ├── Dictionary
        └── Source Code Library
            └── Classes
                └── ZCL_MYCLASS
```

Each mount has an independent selection and facet policy. Applications can
therefore reproduce the standard ADT views or define their own, such as
mounting selected packages directly at the tree root.

## Adaptive Facet Layers

Facet layers make large selections manageable. A package can contain hundreds
or thousands of directly assigned objects, where grouping entries under folders
such as `Source Code Library/Classes` is useful. For a small selection, however,
an extra folder level may only add navigation overhead.

An adaptive facet level is retained only when the current selection contains at
least a configured number of objects. For example:

```rust
use zadt::RepositoryFacet;
use zvfs::{FacetLevel, FacetPolicy};

let policy = FacetPolicy::new([
    FacetLevel::always(RepositoryFacet::OWNER),
    FacetLevel::always(RepositoryFacet::GROUP),
    FacetLevel::adaptive(RepositoryFacet::TYPE, 30),
]);
```

Here the `TYPE` layer, which produces folders such as `Classes`, `Programs`, or
`Database Tables`, is omitted when a group contains fewer than 30 objects. The
tree can then become:

```text
A4H
└── Local Objects ($TMP)
    ├── DEVELOPER
    │   ├── Dictionary                 (TYPE layer omitted)
    │   │   └── ZMYTAB
    │   └── BSP Library
    └── JONDOE
        ├── Dictionary
        └── Source Code Library        (TYPE layer omitted)
            └── ZCL_MYCLASS
```

Adaptive decisions are evaluated independently at each configured level and
are reevaluated when a node is refreshed.

## Example: Command line system explorer

The crate includes a small, interactive explorer example for navigating a live system.
It reads connection details from `.env` or the process environment:

```text
SAP_DESTINATION=https://example.test
SAP_CLIENT=001
SAP_USERNAME=DEVELOPER
SAP_PASSWORD=secret
SAP_LANGUAGE=EN
```

Without an argument, the explorer mounts the System Library. Pass a package
name to start with a narrower package mount:

```bash
cargo run -p zvfs --example explorer
cargo run -p zvfs --example explorer -- /DMO/FLIGHT_REUSE
```

The REPL loads nodes only as they are visited and supports:

```text
ls                 list current children
cd <index>         enter a numbered directory
cd .. | up         navigate to the parent
pwd                print the current repository path
info [index]       show current-node or child metadata
refresh            refresh the current node
tree               render loaded branches and collapse unopened packages
help               show commands
quit | exit        exit
```
This is only an example, it is neither polished nor meant to be used in productive environmens!

## Technical Details

Building a tree performs one RIS facet-catalog request. The builder validates
that every configured policy facet is advertised for structuring and retains
the catalog for hierarchy-aware refresh decisions.

The tree loads each directory lazily. The graph lock is held only while reading
or mutating in-memory records and is never held across an ADT request. A
node-local asynchronous lock deduplicates concurrent loads of the same node
without preventing separate branches from loading concurrently.

Each node receives a tree-scoped `NodeId` containing a UUID and a monotonic
numeric index. Live records are stored in a `HashMap` keyed by that index.
Removed IDs remain stale and cannot resolve to newly inserted nodes.

`zvfs` models repository hierarchy only. Repository objects are leaves in this
tree. Source retrieval, editing, persistence, and local-file projection belong
to higher layers.
