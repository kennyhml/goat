use std::{
    env,
    error::Error,
    io::{self, Write},
};

use zadt::{Client, ReqwestTransport};
use zvfs::{Mount, Node, NodeId, NodeKind, VfsError, VirtualRepositoryTree};

const PACKAGE_COLLAPSE_THRESHOLD: usize = 20;

enum RenderEntry<'a> {
    Node(&'a Node),
    OmittedPackages(usize),
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();

    let transport = ReqwestTransport::builder()
        .destination(required_env("SAP_DESTINATION")?)
        .sap_client(required_env("SAP_CLIENT")?)
        .language(env::var("SAP_LANGUAGE").unwrap_or_else(|_| "EN".to_owned()))
        .basic_auth(required_env("SAP_USERNAME")?, required_env("SAP_PASSWORD")?)
        .build()?;

    let client = Client::new(transport).discover().await?;
    let mount = match env::args().nth(1) {
        Some(package) => Mount::package(package),
        None => Mount::system_library("System Library"),
    };
    let tree = VirtualRepositoryTree::builder(client)
        .mount(mount)
        .build()
        .await?;

    println!("Connected. Type `help` for commands.");
    list_children(&tree, tree.root()).await;
    run_repl(&tree).await
}

async fn run_repl(tree: &VirtualRepositoryTree) -> Result<(), Box<dyn Error>> {
    let mut current = tree.root();
    let stdin = io::stdin();

    loop {
        print!("zvfs:{}> ", display_path(tree, current));
        io::stdout().flush()?;

        let mut input = String::new();
        if stdin.read_line(&mut input)? == 0 {
            println!();
            return Ok(());
        }

        let mut args = input.split_whitespace();
        let Some(command) = args.next() else {
            continue;
        };

        match command {
            "ls" => list_children(tree, current).await,
            "cd" => {
                let Some(target) = args.next() else {
                    eprintln!("usage: cd <index> | cd ..");
                    continue;
                };
                if args.next().is_some() {
                    eprintln!("usage: cd <index> | cd ..");
                    continue;
                }

                if target == ".." {
                    current = parent(tree, current);
                    continue;
                }

                let Ok(index) = target.parse::<usize>() else {
                    eprintln!("`{target}` is not a child index");
                    continue;
                };
                let Some(node) = child_at(tree, current, index).await else {
                    continue;
                };
                if node.is_directory() {
                    current = node.id;
                    list_children(tree, current).await;
                } else {
                    print_node(&node);
                }
            }
            "up" => current = parent(tree, current),
            "pwd" => println!("{}", display_path(tree, current)),
            "info" => match args.next() {
                None => {
                    if let Some(node) = tree.node(current) {
                        print_node(&node);
                    }
                }
                Some(index) if args.next().is_none() => match index.parse::<usize>() {
                    Ok(index) => {
                        if let Some(node) = child_at(tree, current, index).await {
                            print_node(&node);
                        }
                    }
                    Err(_) => eprintln!("`{index}` is not a child index"),
                },
                Some(_) => eprintln!("usage: info [index]"),
            },
            "refresh" => match tree.refresh(current).await {
                Ok(children) => print_children(&children),
                Err(error) => eprintln!("refresh failed: {error}"),
            },
            "tree" => match render_compact_tree(tree) {
                Ok(rendered) => println!("{rendered}"),
                Err(error) => eprintln!("cannot render tree: {error}"),
            },
            "help" => print_help(),
            "quit" | "exit" => return Ok(()),
            _ => eprintln!("unknown command `{command}`; type `help` for commands"),
        }
    }
}

async fn list_children(tree: &VirtualRepositoryTree, parent: NodeId) {
    match tree.children(parent).await {
        Ok(children) => print_children(&children),
        Err(error) => eprintln!("cannot list children: {error}"),
    }
}

async fn child_at(tree: &VirtualRepositoryTree, parent: NodeId, index: usize) -> Option<Node> {
    match tree.children(parent).await {
        Ok(children) => match children.into_iter().nth(index) {
            Some(node) => Some(node),
            None => {
                eprintln!("no child at index {index}");
                None
            }
        },
        Err(error) => {
            eprintln!("cannot list children: {error}");
            None
        }
    }
}

fn parent(tree: &VirtualRepositoryTree, current: NodeId) -> NodeId {
    tree.node(current)
        .and_then(|node| node.parent)
        .unwrap_or(current)
}

fn display_path(tree: &VirtualRepositoryTree, current: NodeId) -> String {
    tree.path(current)
        .map(|path| {
            let labels = path
                .iter()
                .skip(1)
                .map(|node| node.label.as_str())
                .collect::<Vec<_>>();
            if labels.is_empty() {
                "/".to_owned()
            } else {
                format!("/{}", labels.join(" > "))
            }
        })
        .unwrap_or_else(|_| "<stale>".to_owned())
}

fn print_children(children: &[Node]) {
    if children.is_empty() {
        println!("(empty)");
        return;
    }

    for (index, node) in children.iter().enumerate() {
        println!(
            "{index:>3}  {:<9} {}{}",
            kind_label(&node.kind),
            node.label,
            kind_detail(&node.kind)
        );
    }
}

fn kind_label(kind: &NodeKind) -> &'static str {
    match kind {
        NodeKind::Root => "root",
        NodeKind::Mount { .. } => "mount",
        NodeKind::Package { .. } => "package",
        NodeKind::Facet { .. } => "facet",
        NodeKind::Object { .. } => "object",
    }
}

fn kind_detail(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Package {
            object_count: Some(count),
            ..
        } => format!(" ({count} objects)"),
        NodeKind::Facet {
            facet,
            value,
            object_count,
            ..
        } => format!(" ({facet}={value}, {object_count} objects)"),
        NodeKind::Object { object } => format!(" ({})", object.object_type),
        _ => String::new(),
    }
}

fn print_node(node: &Node) {
    println!("label:  {}", node.label);
    println!("id:     {:?}", node.id);
    match &node.kind {
        NodeKind::Root => println!("kind:   root"),
        NodeKind::Mount { mount } => println!("kind:   mount ({mount:?})"),
        NodeKind::Package {
            package,
            uri,
            object_count,
        } => {
            println!("kind:   package");
            println!("name:   {package}");
            println!("uri:    {uri}");
            if let Some(count) = object_count {
                println!("objects: {count}");
            }
        }
        NodeKind::Facet {
            facet,
            value,
            object_count,
            has_children_of_same_facet,
        } => {
            println!("kind:   facet");
            println!("facet:  {facet}");
            println!("value:  {value}");
            println!("objects: {object_count}");
            println!("hierarchical children: {has_children_of_same_facet}");
        }
        NodeKind::Object { object } => {
            println!("kind:   object ({})", object.object_type);
            println!("name:   {}", object.name);
            println!("package: {}", object.package);
            println!("uri:    {}", object.uri);
            if let Some(description) = &object.description {
                println!("text:   {description}");
            }
        }
    }
}

fn render_compact_tree(tree: &VirtualRepositoryTree) -> Result<String, VfsError> {
    let root = tree
        .node(tree.root())
        .expect("the root exists for the lifetime of the tree");
    let mut rendered = root.label;
    render_cached_children(tree, root.id, "", &mut rendered)?;
    Ok(rendered)
}

fn render_cached_children(
    tree: &VirtualRepositoryTree,
    parent: NodeId,
    prefix: &str,
    rendered: &mut String,
) -> Result<(), VfsError> {
    let Some(children) = tree.cached_children(parent)? else {
        return Ok(());
    };

    let children = children
        .into_iter()
        .map(|node| {
            let is_unopened_package = matches!(&node.kind, NodeKind::Package { .. })
                && tree.cached_children(node.id)?.is_none();
            Ok((node, is_unopened_package))
        })
        .collect::<Result<Vec<_>, VfsError>>()?;
    let unopened_packages = children
        .iter()
        .filter(|(_, is_unopened)| *is_unopened)
        .count();
    let collapse_packages = unopened_packages > PACKAGE_COLLAPSE_THRESHOLD;
    let mut added_placeholder = false;
    let mut entries = Vec::with_capacity(children.len());

    for (node, is_unopened_package) in &children {
        if collapse_packages && *is_unopened_package {
            if !added_placeholder {
                entries.push(RenderEntry::OmittedPackages(unopened_packages));
                added_placeholder = true;
            }
        } else {
            entries.push(RenderEntry::Node(node));
        }
    }

    for (position, entry) in entries.iter().enumerate() {
        let is_last = position + 1 == entries.len();
        rendered.push('\n');
        rendered.push_str(prefix);
        rendered.push_str(if is_last { "└── " } else { "├── " });

        match entry {
            RenderEntry::Node(node) => {
                rendered.push_str(&node.label);
                let child_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
                render_cached_children(tree, node.id, &child_prefix, rendered)?;
            }
            RenderEntry::OmittedPackages(count) => {
                rendered.push_str(&format!("... ({count} unopened packages)"));
            }
        }
    }

    Ok(())
}

fn print_help() {
    println!(
        "\
ls                 list and lazily load current children
cd <index>         enter a numbered directory
cd .. | up         navigate to the parent
pwd                print the current repository path
info [index]       show current-node or child metadata
refresh            refresh the current node
tree               render loaded branches and collapse unopened packages
help               show this help
quit | exit        exit"
    );
}

fn required_env(name: &str) -> Result<String, io::Error> {
    env::var(name).map_err(|source| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing required environment variable `{name}`: {source}"),
        )
    })
}
