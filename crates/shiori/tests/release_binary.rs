//! Explicit acceptance against the verified, actually distributed executable.
mod common;
use common::release_images as fixtures;
mod release_support;
use base64::{Engine, engine::general_purpose::STANDARD};
use release_support::{Server, binary, command};
use scraper::{Html, Selector};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, net::TcpListener, process::Command};
use time::OffsetDateTime;

#[test]
#[ignore = "requires the verified release executable"]
fn generation_http_reload_recovery_and_process_failures() {
    let root = tempfile::tempdir().unwrap();
    let root_path = root.path().to_str().unwrap();
    let created = command(&["--root", root_path, "generate", "--title", "Release λ"]);
    assert!(created.status.success());
    let path = std::path::PathBuf::from(String::from_utf8(created.stdout).unwrap().trim());
    assert_eq!(
        path.parent().unwrap(),
        root.path().canonicalize().unwrap().join("articles")
    );
    let id = path.file_stem().unwrap().to_str().unwrap();
    assert_eq!(id.parse::<i64>().unwrap().to_string(), id);
    assert!(root.path().join("assets").join(id).is_dir());
    let initial = fs::read_to_string(&path).unwrap();
    assert!(initial.contains("Release λ") && initial.contains("tags: []"));
    let pixel = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==").unwrap();
    fs::write(root.path().join("assets/photo.png"), &pixel).unwrap();
    fs::write(&path, format!("{initial}# Release\n\n![Photo](/assets/photo.png)\n\n$$\nx^2\n$$\n\n```mermaid\nflowchart LR\n A --> B\n```\n")).unwrap();
    let mut server = Server::start(root.path(), "127.0.0.1", None, false);
    let query = format!(
        "{{ post(id:\"{id}\") {{ id title createdAt updatedAt sourcePath blocks {{ id kind html }} assets {{ id kind mimeType content }} }} }}"
    );
    let result = server.query(&query, false);
    assert!(result.get("errors").is_none(), "{result}");
    assert_eq!(result["data"]["post"]["title"], "Release λ");
    let html = result["data"]["post"]["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["html"].as_str().unwrap())
        .collect::<String>();
    let document = Html::parse_fragment(&html);
    let image = document
        .select(&Selector::parse("img").unwrap())
        .next()
        .unwrap();
    assert_eq!(
        image.value().attr("src").unwrap(),
        format!("{}/images/1280/photo.png", server.origin)
    );
    let response = server
        .client
        .get(image.value().attr("src").unwrap())
        .header("origin", "https://reader.example")
        .send()
        .unwrap();
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["access-control-allow-origin"], "*");
    assert_eq!(response.bytes().unwrap().as_ref(), pixel);
    let font = result["data"]["post"]["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["mimeType"] == "font/otf")
        .unwrap();
    assert!(
        STANDARD
            .decode(font["content"].as_str().unwrap())
            .unwrap()
            .starts_with(b"OTTO")
    );
    assert_eq!(
        server.query("{ posts { nodes { id } totalCount } }", true)["data"]["posts"],
        json!({"nodes":[{"id":id}],"totalCount":1})
    );
    let created_at = OffsetDateTime::parse(
        result["data"]["post"]["createdAt"].as_str().unwrap(),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let monthly = format!(
        "postsByMonth(year:{}, month:{}",
        created_at.year(),
        u8::from(created_at.month())
    );
    let month_result = server.query(&format!("{{ {monthly}) {{ nodes {{ id title }} totalCount pageInfo {{ endCursor hasNextPage }} }} }}"), true);
    let page = &month_result["data"]["postsByMonth"];
    assert_eq!(page["nodes"], json!([{"id":id,"title":"Release λ"}]));
    assert_eq!(page["totalCount"], 1);
    assert_eq!(page["pageInfo"]["hasNextPage"], false);
    let cursor = page["pageInfo"]["endCursor"].as_str().unwrap();
    assert!(cursor.starts_with("v1."));
    assert_eq!(server.query(&format!("{{ {monthly}, after:\"{cursor}\") {{ nodes {{ id }} totalCount pageInfo {{ endCursor hasNextPage }} }} }}"), false)["data"]["postsByMonth"],
        json!({"nodes":[],"totalCount":1,"pageInfo":{"endCursor":null,"hasNextPage":false}}));
    assert_eq!(
        server.query(
            "{ postsByMonth(year:2026, month:13) { totalCount } }",
            false
        )["errors"][0]["extensions"]["code"],
        "BAD_USER_INPUT"
    );
    let replacement = root.path().join("articles/.editor-save");
    fs::write(
        &replacement,
        initial.replace("Release λ", "Updated release") + "# Updated body\n",
    )
    .unwrap();
    fs::rename(replacement, &path).unwrap();
    server.event("reload");
    assert_eq!(
        server.query(&query, false)["data"]["post"]["title"],
        "Updated release"
    );
    fs::write(&path, "invalid metadata").unwrap();
    server.event("invalid_article");
    server.event("reload");
    assert_eq!(
        server.query(&query, false)["data"]["post"]["title"],
        "Updated release"
    );
    fs::write(
        &path,
        initial.replace("Release λ", "Recovered release") + "# Recovered\n",
    )
    .unwrap();
    server.event("reload");
    assert_eq!(
        server.query(&query, false)["data"]["post"]["title"],
        "Recovered release"
    );
    server.close();
    let restarted = Server::start(root.path(), "127.0.0.1", None, false);
    assert_eq!(
        restarted.query(&query, false)["data"]["post"]["title"],
        "Recovered release"
    );
    restarted.close();
    let entries: BTreeSet<_> = fs::read_dir(root.path())
        .unwrap()
        .map(|p| p.unwrap().file_name())
        .collect();
    assert_eq!(
        entries,
        ["articles", "assets"].map(std::ffi::OsString::from).into()
    );
    assert!(
        !fs::read_dir(root.path().join("articles"))
            .unwrap()
            .any(|p| p.unwrap().path().extension().is_some_and(|e| e == "tmp"))
    );
    let missing = root.path().join("missing");
    for arguments in [
        vec!["--root", missing.to_str().unwrap(), "generate"],
        vec!["--root", root_path, "generate", "--title", ""],
        vec![
            "--root",
            root_path,
            "serve",
            "--public-base-url",
            "ftp://invalid",
        ],
        vec!["--root", root_path, "serve", "--bind", "invalid"],
    ] {
        let output = command(&arguments);
        assert!(!output.status.success() && !output.stderr.is_empty() && output.stdout.is_empty());
    }
    fs::write(&path, "invalid startup").unwrap();
    let output = command(&["--root", root_path, "serve"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("invalid article")
    );
    fs::write(&path, initial).unwrap();
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let output = command(&[
        "--root",
        root_path,
        "serve",
        "--bind",
        &occupied.local_addr().unwrap().to_string(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8(output.stderr).unwrap().contains("fatal"));
}

#[test]
#[ignore = "requires the verified release executable"]
fn graphiql_and_encoded_ipv4_ipv6_image_urls() {
    for (enabled, prefix) in [(false, ""), (true, ""), (true, "/blog/v1")] {
        let root = tempfile::tempdir().unwrap();
        common::article(root.path(), 1, "GraphiQL smoke", &[], "Body");
        let public = format!("https://cms.example{prefix}");
        let server = Server::start(root.path(), "127.0.0.1", Some(&public), enabled);
        if enabled {
            let response = server.request("/graphiql");
            assert_eq!(response.status(), 200);
            assert!(
                response.headers()["content-security-policy"]
                    .to_str()
                    .unwrap()
                    .contains("connect-src 'self'")
            );
            let html = response.text().unwrap();
            let document = Html::parse_document(&html);
            assert_eq!(
                document
                    .select(&Selector::parse("[data-endpoint]").unwrap())
                    .next()
                    .unwrap()
                    .value()
                    .attr("data-endpoint")
                    .unwrap(),
                format!("{prefix}/graphql")
            );
            let resources: Vec<_> = document
                .select(
                    &Selector::parse("script[src],link[rel=stylesheet],link[rel=license]").unwrap(),
                )
                .collect();
            assert_eq!(resources.len(), 3);
            for resource in resources {
                let path = resource
                    .value()
                    .attr("src")
                    .or_else(|| resource.value().attr("href"))
                    .unwrap();
                assert!(path.starts_with(&format!("{prefix}/graphiql/assets/")));
                let response = server.request(path.strip_prefix(prefix).unwrap());
                assert_eq!(response.status(), 200);
                assert!(!response.bytes().unwrap().is_empty());
            }
            let schema = server.query(
                include_str!("fixtures/graphiql-introspection.graphql"),
                false,
            );
            assert!(schema.get("errors").is_none(), "{schema}");
            assert!(
                schema["data"]["__schema"]["types"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v["name"] == "Post")
            );
        } else {
            for path in ["/graphiql", "/graphiql/assets/app.js"] {
                assert_eq!(server.request(path).status(), 404);
            }
        }
        assert_eq!(
            server.query("{ post(id:\"1\") { title } }", false)["data"]["post"]["title"],
            "GraphiQL smoke"
        );
        server.close();
    }
    for host in ["127.0.0.1", "::1"] {
        let root = tempfile::tempdir().unwrap();
        let pixel = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==").unwrap();
        fs::create_dir(root.path().join("assets")).unwrap();
        fs::write(root.path().join("assets/pixel [λ].png"), &pixel).unwrap();
        let path = "/assets/pixel%20%5B%CE%BB%5D.png";
        common::article(
            root.path(),
            1,
            "Image URL",
            &[],
            &format!("![a]({path})\n\n![b]({path}?size=1&mode=raw#pixel)"),
        );
        let server = Server::start(root.path(), host, None, false);
        assert_eq!(server.request(path).bytes().unwrap().as_ref(), pixel);
        let result = server.query("{ post(id:\"1\") { blocks { html } } }", false);
        assert!(result.get("errors").is_none(), "{result}");
        let html = result["data"]["post"]["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["html"].as_str().unwrap())
            .collect::<String>();
        let document = Html::parse_fragment(&html);
        let images: Vec<_> = document.select(&Selector::parse("img").unwrap()).collect();
        assert_eq!(images.len(), 2);
        for (image, suffix) in images.iter().zip(["", "?size=1&mode=raw#pixel"]) {
            let source = image.value().attr("src").unwrap();
            assert_eq!(
                source,
                format!(
                    "{}{}{suffix}",
                    server.origin,
                    path.replace("/assets/", "/images/1280/")
                )
            );
            let response = server
                .client
                .get(source)
                .header("origin", "https://reader.example")
                .send()
                .unwrap();
            assert_eq!(response.headers()["content-type"], "image/png");
            assert_eq!(response.headers()["access-control-allow-origin"], "*");
            assert_eq!(response.bytes().unwrap().as_ref(), pixel);
        }
        server.close();
    }
}

#[test]
#[ignore = "requires the verified release executable"]
fn responsive_release_bytes() {
    for (host, prefix) in [("127.0.0.1", ""), ("::1", ""), ("127.0.0.1", "/blog/v1")] {
        let root = tempfile::tempdir().unwrap();
        fixtures::export(root.path());
        let _read_only = release_support::ReadOnlyRoot::new(root.path());
        let proxy = (!prefix.is_empty()).then(release_support::proxy::Proxy::start);
        let public = proxy.as_ref().map(|p| format!("{}{prefix}", p.origin));
        let server = Server::start(root.path(), host, public.as_deref(), false);
        if let Some(proxy) = &proxy {
            proxy.target(&server.origin);
        }
        let base = public.unwrap_or_else(|| server.origin.clone());
        let result: Value = server
            .client
            .post(format!("{base}/graphql"))
            .header("origin", "https://reader.example")
            .header("x-chigiri-preflight", "1")
            .json(&json!({"query":"{ post(id:\"1\") { blocks { html } } }"}))
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert!(result.get("errors").is_none(), "{result}");
        let html = result["data"]["post"]["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["html"].as_str().unwrap())
            .collect::<String>();
        let document = Html::parse_fragment(&html);
        let images: Vec<_> = document.select(&Selector::parse("img").unwrap()).collect();
        assert_eq!(images.len(), 15);
        let mut unique = BTreeSet::new();
        for image in images {
            let element = image.value();
            let Some(srcset) = element.attr("srcset") else {
                let name = match element.attr("alt").unwrap() {
                    "Vector" => "vector.svg",
                    "Gif" => "tiny.gif",
                    alt => panic!("unexpected fallback: {alt}"),
                };
                assert_eq!(
                    element.attr("src").unwrap(),
                    format!("{base}/assets/{name}")
                );
                assert_eq!(
                    server
                        .client
                        .get(element.attr("src").unwrap())
                        .send()
                        .unwrap()
                        .bytes()
                        .unwrap()
                        .as_ref(),
                    fs::read(root.path().join("assets").join(name)).unwrap()
                );
                continue;
            };
            let kind = match element.attr("alt").unwrap() {
                "Alpha" => "alpha",
                "Tiny" => "tiny",
                "Portrait" => "portrait",
                "Oriented" => "oriented",
                "Height bound" => "bounded",
                _ => "wide",
            };
            let width: u32 = element.attr("width").unwrap().parse().unwrap();
            let height: u32 = element.attr("height").unwrap().parse().unwrap();
            assert_eq!(element.attr("sizes"), Some("100vw"));
            // URLs may contain commas; parse the whitespace-separated descriptor boundary.
            let mut candidates = Vec::new();
            let mut tokens = srcset.split_ascii_whitespace();
            while let Some(url) = tokens.next() {
                let descriptor = tokens
                    .next()
                    .unwrap()
                    .trim_end_matches(',')
                    .strip_suffix('w')
                    .unwrap();
                candidates.push((url, descriptor.parse::<u32>().unwrap()));
            }
            let mut expected: Vec<_> = [320, 640, 960, 1280, 1920, 2560]
                .into_iter()
                .filter(|w| *w < width.min(2560))
                .collect();
            expected.push(width.min(2560));
            assert_eq!(
                candidates.iter().map(|(_, w)| *w).collect::<Vec<_>>(),
                expected
            );
            candidates.push((element.attr("src").unwrap(), width.min(1280)));
            for (url, pixels) in candidates {
                assert!(url.starts_with(&format!("{base}/images/")), "{url}");
                let parsed = url::Url::parse(url).unwrap();
                if kind == "wide" {
                    assert_eq!(parsed.query(), Some("authored=yes&width=420"));
                    assert_eq!(parsed.fragment(), Some("photo"));
                    assert!(parsed.path().contains("%5B%CE%BB%5D"));
                }
                for (accept, mime) in [
                    ("image/webp", "image/webp"),
                    (
                        "*/*",
                        if ["wide", "oriented"].contains(&kind) {
                            "image/jpeg"
                        } else {
                            "image/png"
                        },
                    ),
                ] {
                    if !unique.insert((url.to_owned(), accept)) {
                        continue;
                    }
                    let response = server
                        .client
                        .get(url)
                        .header("accept", accept)
                        .header("origin", "https://reader.example")
                        .send()
                        .unwrap();
                    assert_eq!(response.status(), 200);
                    assert_eq!(response.headers()["content-type"], mime);
                    assert_eq!(response.headers()["access-control-allow-origin"], "*");
                    assert_eq!(response.headers()["cache-control"], "no-cache");
                    assert!(
                        response.headers()["vary"]
                            .to_str()
                            .unwrap()
                            .contains("Accept")
                    );
                    let bytes = response.bytes().unwrap();
                    fixtures::verify_download(
                        &bytes,
                        &json!({"width":pixels,"height":((u64::from(height)*u64::from(pixels)+u64::from(width/2))/u64::from(width)).max(1),"kind":kind}),
                    );
                }
            }
        }
        assert!(!unique.is_empty());
        server.close();
    }
    assert!(binary().is_file());
}

#[test]
#[ignore = "requires a built executable and Playwright"]
fn responsive_browser_layout() {
    let root = tempfile::tempdir().unwrap();
    fixtures::export(root.path());
    let _read_only = release_support::ReadOnlyRoot::new(root.path());
    let server = Server::start(root.path(), "127.0.0.1", None, false);
    let status = Command::new("npm")
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args([
            "exec",
            "--prefix",
            "tools/browser",
            "--",
            "playwright",
            "test",
            "--config",
            "tools/browser/playwright.config.ts",
        ])
        .env("SHIORI_ACCEPTANCE_CMS", &server.origin)
        .status()
        .unwrap();
    assert!(status.success(), "Real browser acceptance failed");
    server.close();
}

#[test]
#[ignore = "requires a built executable; release acceptance selects the verified artifact"]
fn distribution_identity_and_component_scope() {
    let machine = command(&["--machine-version"]);
    assert!(machine.status.success() && machine.stderr.is_empty());
    let identity: Value = serde_json::from_slice(&machine.stdout).unwrap();
    let output = command(&["sbom"]);
    assert!(output.status.success() && output.stderr.is_empty());
    let sbom: Value = serde_json::from_slice(&output.stdout).unwrap();
    let embedded: Value = serde_json::from_str(
        sbom["metadata"]["properties"]
            .as_array()
            .unwrap()
            .iter()
            .find(|property| property["name"] == "craftguild:build")
            .unwrap()["value"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(identity, embedded);
    assert_eq!(identity["version"], env!("CARGO_PKG_VERSION"));
    let names: BTreeSet<_> = sbom["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|component| component["name"].as_str().unwrap())
        .collect();
    for required in [
        "chigiri_graphiql",
        "oriha-core",
        "@graphiql/react",
        "monaco-editor",
        "STIX Two Math",
        "libwebp",
    ] {
        assert!(
            names.contains(required),
            "Missing actual embedded component: {required}"
        );
    }
    for excluded in [
        "next",
        "shiori-frontend",
        "lab06-blog-frontend",
        "@playwright/test",
        "scraper",
        "reqwest",
        "craftguild-cli-build",
    ] {
        assert!(
            !names.contains(excluded),
            "Non-distributed component included: {excluded}"
        );
    }
}
