use std::io;

use tokio::fs;

use crate::{
    app::{AppError, AppState},
    auth::User,
    config::EffectiveOptions,
    storage::{encode_path, join_checked, list_directory, validate_system_path},
};

#[derive(Debug)]
struct BrowseLink {
    href: String,
    label: String,
    is_dir: bool,
}

pub(crate) async fn render_browse_page(
    state: &AppState,
    user: &User,
    raw_path: &str,
) -> Result<String, AppError> {
    if !raw_path.is_empty() {
        validate_system_path(&state.systems, raw_path)?;
    }
    let dir = join_checked(&state.roms_path, raw_path)?;
    let mut dir_entries = fs::read_dir(&dir).await.map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => AppError::NotFound,
        _ => err.into(),
    })?;

    let mut out = Vec::new();
    while let Some(entry) = dir_entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if file_type.is_dir() {
            if list_directory(&state.systems, raw_path, &name) {
                out.push(BrowseLink {
                    href: content_href(&join_path(raw_path, &name)),
                    label: format!("{name}/"),
                    is_dir: true,
                });
            }
        } else if file_type.is_file()
            && state
                .systems
                .for_path(raw_path)
                .is_some_and(|system| state.systems.supports_file(system, &entry.path()))
        {
            out.push(BrowseLink {
                href: content_href(&join_path(raw_path, &name)),
                label: name,
                is_dir: false,
            });
        }
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });

    let mut rows = String::new();
    for entry in out {
        let class = if entry.is_dir { "row dir" } else { "row" };
        rows.push_str(&format!(
            "<div class=\"{class}\"><a href=\"{}\">{}</a></div>",
            escape_html(&entry.href),
            escape_html(&entry.label)
        ));
    }

    Ok(format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>BARP</title>
    <style>
      :root {{
        color-scheme: light dark;
        --background: light-dark(#f5f6f8, #111315);
        --surface: light-dark(#ffffff, #1b1e21);
        --border: light-dark(#d8dce2, #34383d);
        --text: light-dark(#20242a, #e7e9ec);
        --muted: light-dark(#626a75, #a8afb8);
        --accent: light-dark(#245fbd, #78aaf7);
      }}
      * {{ box-sizing: border-box; }}
      body {{
        margin: 0;
        background: var(--background);
        color: var(--text);
        font-family: system-ui, sans-serif;
        line-height: 1.5;
      }}
      main {{ width: min(48rem, calc(100% - 1rem)); margin: .5rem auto; }}
      h1 {{ margin: .5rem 0 0; font-size: 1.75rem; }}
      .toolbar {{
        display: flex;
        gap: 1rem;
        align-items: center;
        padding-bottom: .5rem;
        border-bottom: 1px solid var(--border);
      }}
      .toolbar strong {{ font-size: 1.15rem; }}
      .toolbar span {{ color: var(--muted); }}
      .toolbar form {{ margin-left: auto; }}
      button, input {{
        border: 1px solid var(--border);
        border-radius: .4rem;
        background: var(--surface);
        color: var(--text);
        font: inherit;
      }}
      button {{ padding: .4rem .75rem; cursor: pointer; }}
      button:hover {{ border-color: var(--accent); }}
      section {{
        overflow: hidden;
        border: 1px solid var(--border);
        border-radius: .4rem;
        background: var(--surface);
      }}
      .row a {{
        display: block;
        padding: .4rem .7rem;
        border-bottom: 1px solid var(--border);
        color: var(--text);
        text-decoration: none;
      }}
      .row:last-child a {{ border-bottom: 0; }}
      .row a:hover {{ background: var(--background); color: var(--accent); }}
      .path {{ margin: 0 0 .5rem; color: var(--muted); }}
      .path a {{ color: inherit; text-decoration: none; }}
      .path a:hover {{ color: var(--accent); text-decoration: underline; }}
    </style>
  </head>
  <body>
    <main>
      <div class="toolbar">
        <strong>BARP</strong>
        <span>{user}</span>
        <form method="post" action="/logout">
          <button type="submit">Log out</button>
        </form>
      </div>
      <h1>Library</h1>
      <p class="path">{crumbs}</p>
      <section>{rows}</section>
    </main>
  </body>
</html>"#,
        user = escape_html(&user.display_name),
        crumbs = path_crumbs(raw_path),
        rows = rows,
    ))
}

pub(crate) fn render_play_page(
    path: &str,
    save_path: &str,
    core: &str,
    options: &EffectiveOptions,
    has_save: bool,
    threads: bool,
) -> String {
    let smooth = if options.smooth { "1" } else { "0" };
    let integer_scale = if options.integer_scale { "1" } else { "0" };
    format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>BARP - {path}</title>
    <style>
      html, body {{ width: 100%; height: 100%; margin: 0; background: #000; overflow: hidden; }}
      #game {{ width: 100%; height: 100dvh; }}
      #game .ejs_parent,
      #game .ejs_canvas_parent {{
        display: flex;
        align-items: center;
        justify-content: center;
      }}
    </style>
  </head>
  <body data-path="{path}" data-save-path="{save_path}" data-core="{core}" data-shader="{shader}" data-smooth="{smooth}" data-integer-scale="{integer_scale}" data-has-save="{has_save}" data-threads="{threads}">
    <div id="game"></div>
    <script src="/player.js"></script>
  </body>
</html>"#,
        path = escape_html(path),
        save_path = escape_html(save_path),
        core = escape_html(core),
        shader = escape_html(&options.shader),
        smooth = smooth,
        integer_scale = integer_scale,
        has_save = if has_save { "1" } else { "0" },
        threads = if threads { "1" } else { "0" },
    )
}

pub(crate) fn render_login_page(next: &str, error: Option<&str>) -> String {
    let error_html = error
        .map(|message| format!("<p class=\"error\">{}</p>", escape_html(message)))
        .unwrap_or_default();
    format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>BARP - Login</title>
    <style>
      :root {{
        color-scheme: light dark;
        --background: light-dark(#f5f6f8, #111315);
        --surface: light-dark(#ffffff, #1b1e21);
        --border: light-dark(#d8dce2, #34383d);
        --text: light-dark(#20242a, #e7e9ec);
        --muted: light-dark(#626a75, #a8afb8);
        --accent: light-dark(#245fbd, #78aaf7);
        --error: light-dark(#b42318, #ff8a80);
      }}
      * {{ box-sizing: border-box; }}
      body {{
        display: grid;
        min-height: 100vh;
        margin: 0;
        place-items: center;
        background: var(--background);
        color: var(--text);
        font-family: system-ui, sans-serif;
        line-height: 1.5;
      }}
      main {{ width: min(24rem, calc(100% - 2rem)); }}
      h1 {{ margin-top: 0; text-align: center; }}
      form {{
        padding: 1.25rem;
        border: 1px solid var(--border);
        border-radius: .6rem;
        background: var(--surface);
      }}
      label {{ display: grid; gap: .25rem; margin-bottom: 1rem; color: var(--muted); }}
      input, button {{
        width: 100%;
        padding: .6rem .7rem;
        border: 1px solid var(--border);
        border-radius: .4rem;
        background: var(--background);
        color: var(--text);
        font: inherit;
      }}
      input:focus {{ border-color: var(--accent); outline: 2px solid var(--accent); outline-offset: 1px; }}
      button {{ background: var(--accent); color: light-dark(#ffffff, #101214); cursor: pointer; font-weight: 600; }}
      .error {{ color: var(--error); }}
    </style>
  </head>
  <body>
    <main>
      <h1>BARP</h1>
      {error_html}
      <form method="post" action="/login">
        <input type="hidden" name="next" value="{next}">
        <label>Username <input name="username" autocomplete="username" required></label>
        <label>Password <input name="password" type="password" autocomplete="current-password" required></label>
        <button type="submit">Log in</button>
      </form>
    </main>
  </body>
</html>"#,
        error_html = error_html,
        next = escape_html(next),
    )
}

pub(crate) fn content_href(path: &str) -> String {
    let path = normalize_content_path(path);
    if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", encode_path(&path))
    }
}

pub(crate) fn normalize_content_path(path: &str) -> String {
    path.split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn path_crumbs(path: &str) -> String {
    let path = normalize_content_path(path);
    let mut html = format!("<a href=\"{}\">roms</a>", escape_html(&content_href("")));
    if path.is_empty() {
        return html;
    }

    let mut accumulated = String::new();
    for segment in path.split('/') {
        html.push_str(" / ");
        if !accumulated.is_empty() {
            accumulated.push('/');
        }
        accumulated.push_str(segment);
        html.push_str(&format!(
            "<a href=\"{}\">{}</a>",
            escape_html(&content_href(&accumulated)),
            escape_html(segment)
        ));
    }
    html
}

fn join_path(base: &str, child: &str) -> String {
    let base = normalize_content_path(base);
    if base.is_empty() {
        child.to_string()
    } else {
        format!("{base}/{child}")
    }
}

fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_links_use_the_filesystem_path_directly() {
        assert_eq!(content_href(""), "/");
        assert_eq!(content_href("nes"), "/nes");
        assert_eq!(
            content_href("nes/Super Mario Bros.nes"),
            "/nes/Super%20Mario%20Bros.nes"
        );
    }

    #[test]
    fn normalize_content_path_strips_slashes_and_empty_segments() {
        assert_eq!(normalize_content_path("nes/"), "nes");
        assert_eq!(normalize_content_path("/nes/"), "nes");
        assert_eq!(normalize_content_path("nes//mario"), "nes/mario");
        assert_eq!(join_path("nes/", "mario"), "nes/mario");
        assert_eq!(content_href("nes/"), "/nes");
    }

    #[test]
    fn path_crumbs_link_each_directory_segment() {
        assert_eq!(path_crumbs(""), "<a href=\"/\">roms</a>");
        assert_eq!(
            path_crumbs("nes"),
            "<a href=\"/\">roms</a> / <a href=\"/nes\">nes</a>"
        );
        assert_eq!(
            path_crumbs("nes/Mario"),
            "<a href=\"/\">roms</a> / <a href=\"/nes\">nes</a> / <a href=\"/nes/Mario\">Mario</a>"
        );
    }
}
