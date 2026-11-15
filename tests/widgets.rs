use dost::theme::Theme;
use dost::widgets::{
    Cell, ChainWidget, GraphWidget, LinkState, MapLink, MapNode, MapWidget, NodeStatus,
    ProgressWidget, TableWidget,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

fn render(w: impl Widget, width: u16, height: u16) -> Buffer {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    w.render(area, &mut buf);
    buf
}

fn text(buf: &Buffer) -> String {
    let a = buf.area;
    (0..a.height)
        .map(|y| {
            (0..a.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn node(id: &str, st: NodeStatus) -> MapNode {
    MapNode {
        id: id.into(),
        label: id.into(),
        status: st,
    }
}

#[test]
fn chain_renders_status_glyphs() {
    let w = ChainWidget {
        steps: vec![
            ("base".into(), NodeStatus::Ok),
            ("wg".into(), NodeStatus::Running),
            ("k3s".into(), NodeStatus::Pending),
            ("argo".into(), NodeStatus::Failed),
        ],
        theme: Theme::default(),
    };
    let t = text(&render(w, 80, 3));
    for s in ["✓ base", "wg", "○ k3s", "✗ argo"] {
        assert!(t.contains(s), "missing {s}:\n{t}");
    }
}

#[test]
fn chain_goes_vertical_when_narrow() {
    let w = ChainWidget {
        steps: vec![
            ("base".into(), NodeStatus::Ok),
            ("wireguard".into(), NodeStatus::Ok),
            ("kubernetes".into(), NodeStatus::Pending),
        ],
        theme: Theme::default(),
    };
    let buf = render(w, 14, 5);
    let t = text(&buf);
    assert!(
        t.lines()
            .filter(|l| l.contains('✓') || l.contains('○'))
            .count()
            >= 3,
        "{t}"
    );
}

#[test]
fn table_changed_cell_is_highlighted() {
    let w = TableWidget {
        columns: vec!["port".into(), "was".into(), "now".into()],
        rows: vec![vec![
            Cell {
                text: "80".into(),
                changed: false,
            },
            Cell {
                text: "ON".into(),
                changed: false,
            },
            Cell {
                text: "off".into(),
                changed: true,
            },
        ]],
        highlight_changed: true,
        theme: Theme::default(),
    };
    let buf = render(w, 30, 4);
    let t = text(&buf);
    assert!(t.contains("port") && t.contains("off"), "{t}");
    let find = |needle: &str| {
        let a = buf.area;
        for y in 0..a.height {
            let row: String = (0..a.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            if let Some(i) = row.find(needle) {
                return buf[(i as u16, y)].style();
            }
        }
        panic!("{needle} not found");
    };
    assert_ne!(
        find("off").fg,
        find("ON").fg,
        "changed cell must have another colour"
    );
}

#[test]
fn map_with_down_link_shows_break_marker() {
    let w = MapWidget {
        nodes: vec![
            node("ops", NodeStatus::Ok),
            node("hero", NodeStatus::Ok),
            node("mail", NodeStatus::Pending),
        ],
        links: vec![
            MapLink {
                a: "ops".into(),
                b: "hero".into(),
                state: LinkState::Up,
            },
            MapLink {
                a: "hero".into(),
                b: "mail".into(),
                state: LinkState::Down,
            },
        ],
        theme: Theme::default(),
    };
    let t = text(&render(w, 80, 8));
    assert!(t.contains("═"), "up link must be drawn:\n{t}");
    assert!(t.contains("✗"), "down link must show a break marker:\n{t}");
    assert!(t.contains("mail"));
}

#[test]
fn small_area_falls_back_to_list() {
    let w = MapWidget {
        nodes: vec![
            node("ops", NodeStatus::Ok),
            node("hero", NodeStatus::Ok),
            node("mail", NodeStatus::Ok),
        ],
        links: vec![MapLink {
            a: "ops".into(),
            b: "hero".into(),
            state: LinkState::Up,
        }],
        theme: Theme::default(),
    };
    let t = text(&render(w, 20, 4));
    assert!(!t.contains("═"), "{t}");
    assert!(t.contains("ops") && t.contains("hero"), "{t}");
}

#[test]
fn fifty_one_nodes_fall_back_to_list() {
    let nodes: Vec<MapNode> = (0..51)
        .map(|i| node(&format!("n{i}"), NodeStatus::Ok))
        .collect();
    let w = MapWidget {
        nodes,
        links: vec![],
        theme: Theme::default(),
    };
    let t = text(&render(w, 200, 60));
    assert!(!t.contains("╔") && !t.contains("┌"), "{t}");
    assert!(t.contains("n50"), "{t}");
}

#[test]
fn graph_lists_dependencies() {
    let w = GraphWidget {
        nodes: vec![
            node("base", NodeStatus::Ok),
            node("wg", NodeStatus::Pending),
        ],
        edges: vec![("wg".into(), "base".into())],
        theme: Theme::default(),
    };
    let t = text(&render(w, 40, 4));
    assert!(t.contains("wg") && t.contains("base"), "{t}");
}

#[test]
fn progress_shows_ratio_and_label() {
    let w = ProgressWidget {
        step: 3,
        of: 6,
        label: "preprod".into(),
        theme: Theme::default(),
    };
    let t = text(&render(w, 40, 1));
    assert!(t.contains("3/6") && t.contains("preprod"), "{t}");
}

#[test]
fn theme_reads_colours_from_catalog_with_fallback() {
    use dost::catalog::Theme as CatTheme;
    let mut c = CatTheme::default();
    c.colors.insert("ok".into(), "blue".into());
    c.colors.insert("fail".into(), "not-a-colour".into());
    let t = Theme::from_catalog(&c);
    assert_eq!(t.ok, ratatui::style::Color::Blue);
    assert_eq!(t.fail, Theme::default().fail);
}
