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

use dost::widgets::{TreeNode, TreeWidget};

fn tnode(id: &str, parent: Option<&str>, st: NodeStatus) -> TreeNode {
    TreeNode {
        id: id.into(),
        label: id.into(),
        parent: parent.map(String::from),
        status: st,
    }
}

fn sample_tree(frame: u64, agent: NodeStatus) -> TreeWidget {
    TreeWidget {
        nodes: vec![
            tnode("openbao", None, NodeStatus::Ok),
            tnode("hero", Some("openbao"), NodeStatus::Unknown),
            tnode("agent-stage", Some("hero"), NodeStatus::Ok),
            tnode("agent-prod", Some("hero"), agent),
            tnode("mail", Some("openbao"), NodeStatus::Pending),
        ],
        frame,
        theme: Theme::default(),
    }
}

#[test]
fn tree_draws_connectors_hierarchy_and_status_glyphs() {
    let t = text(&render(sample_tree(0, NodeStatus::Failed), 60, 10));
    let lines: Vec<&str> = t.lines().collect();
    assert!(
        lines[0].contains("openbao") && lines[0].contains('✓'),
        "{t}"
    );
    assert!(
        lines[1].starts_with("├─") && lines[1].contains("hero"),
        "{t}"
    );
    assert!(
        lines[2].starts_with("│  ├─") && lines[2].contains("agent-stage"),
        "{t}"
    );
    assert!(
        lines[3].starts_with("│  └─") && lines[3].contains('✗'),
        "{t}"
    );
    assert!(
        lines[4].starts_with("└─") && lines[4].contains("mail") && lines[4].contains('○'),
        "{t}"
    );
    assert!(t.contains('·'), "unknown status is a dot:\n{t}");
}

#[test]
fn tree_spinner_changes_with_the_frame() {
    let a = text(&render(sample_tree(0, NodeStatus::Running), 60, 10));
    let b = text(&render(sample_tree(3, NodeStatus::Running), 60, 10));
    assert_ne!(a, b, "a running node must animate");
    let still_a = text(&render(sample_tree(0, NodeStatus::Ok), 60, 10));
    let still_b = text(&render(sample_tree(3, NodeStatus::Ok), 60, 10));
    assert_eq!(still_a, still_b, "finished nodes must not animate");
}

#[test]
fn tree_clips_in_a_tiny_area_without_panicking() {
    let _ = render(sample_tree(1, NodeStatus::Running), 6, 2);
    let _ = render(sample_tree(1, NodeStatus::Running), 0, 0);
}

#[test]
fn tree_with_unknown_parent_shows_the_node_at_the_top() {
    let w = TreeWidget {
        nodes: vec![tnode("orphan", Some("missing"), NodeStatus::Ok)],
        frame: 0,
        theme: Theme::default(),
    };
    assert!(text(&render(w, 30, 3)).contains("orphan"));
}

use dost::widgets::{PipelineLink, PipelineWidget};

fn plink(label: &str, st: NodeStatus, detail: Option<&str>) -> PipelineLink {
    PipelineLink {
        label: label.into(),
        status: st,
        detail: detail.map(String::from),
    }
}

fn pipeline(frame: u64, second: NodeStatus) -> PipelineWidget {
    PipelineWidget {
        links: vec![
            plink("Host", NodeStatus::Ok, Some("tunnel active")),
            plink("Ports", second, Some("8000 ✓  8002 ✓")),
            plink("GPU host", NodeStatus::Pending, None),
        ],
        frame,
        theme: Theme::default(),
    }
}

#[test]
fn pipeline_draws_links_connectors_and_details() {
    let t = text(&render(pipeline(0, NodeStatus::Ok), 60, 8));
    let lines: Vec<&str> = t.lines().collect();
    assert!(
        lines[0].contains('✓') && lines[0].contains("Host") && lines[0].contains("tunnel active"),
        "{t}"
    );
    assert!(
        lines[1].trim_start().starts_with('│'),
        "connector between links:\n{t}"
    );
    assert!(
        lines[2].contains("Ports") && lines[2].contains("8000 ✓  8002 ✓"),
        "{t}"
    );
    assert!(
        lines[4].contains('○') && lines[4].contains("GPU host"),
        "{t}"
    );
}

#[test]
fn pipeline_spinner_only_animates_the_running_link() {
    let a = text(&render(pipeline(0, NodeStatus::Running), 60, 8));
    let b = text(&render(pipeline(4, NodeStatus::Running), 60, 8));
    assert_ne!(a, b);
    let c = text(&render(pipeline(0, NodeStatus::Ok), 60, 8));
    let d = text(&render(pipeline(4, NodeStatus::Ok), 60, 8));
    assert_eq!(c, d);
}

#[test]
fn pipeline_clips_without_panicking_in_tiny_areas() {
    let _ = render(pipeline(1, NodeStatus::Running), 5, 2);
    let _ = render(pipeline(1, NodeStatus::Running), 0, 0);
}

use dost::widgets::{TransferRow, TransferWidget};

fn trow(
    label: &str,
    st: NodeStatus,
    done: Option<u64>,
    total: Option<u64>,
    detail: Option<&str>,
    age: u64,
) -> TransferRow {
    TransferRow {
        label: label.into(),
        from: "kv/prod/backend".into(),
        to: "kv/preprod/backend".into(),
        status: st,
        done,
        total,
        detail: detail.map(String::from),
        age,
    }
}

fn transfer(frame: u64, rows: Vec<TransferRow>) -> TransferWidget {
    TransferWidget {
        titles: ("PROD".into(), "PREPROD".into()),
        rows,
        frame,
        theme: Theme::default(),
    }
}

#[test]
fn transfer_shows_titles_from_to_and_a_megabyte_bar() {
    let w = transfer(
        0,
        vec![trow(
            "files",
            NodeStatus::Running,
            Some(120 * 1024 * 1024),
            Some(480 * 1024 * 1024),
            None,
            99,
        )],
    );
    let t = text(&render(w, 100, 4));
    assert!(
        t.lines().next().unwrap().contains("PROD") && t.lines().next().unwrap().contains("PREPROD"),
        "{t}"
    );
    assert!(
        t.contains("kv/prod/backend") && t.contains("kv/preprod/backend"),
        "{t}"
    );
    assert!(
        t.contains("120.0 MB") && t.contains("480.0 MB"),
        "bytes shown as done / max:\n{t}"
    );
    assert!(
        t.contains('█') && t.contains('░'),
        "a bar with a filled and an empty part:\n{t}"
    );
}

#[test]
fn transfer_done_row_is_ticked_and_missing_row_is_crossed() {
    let w = transfer(
        0,
        vec![
            trow("A", NodeStatus::Ok, None, None, None, 99),
            trow("B", NodeStatus::Failed, None, None, Some("MISSING"), 99),
        ],
    );
    let t = text(&render(w, 100, 5));
    let a = t.lines().find(|l| l.contains(" A ")).unwrap();
    let b = t.lines().find(|l| l.contains(" B ")).unwrap();
    assert!(a.contains('✓') && a.contains("═") && a.contains('▶'), "{t}");
    assert!(
        b.contains('✗') && b.contains("MISSING") && b.contains('╳'),
        "{t}"
    );
}

#[test]
fn transfer_packet_moves_with_the_frame_while_running_and_on_arrival() {
    let running = |f| {
        text(&render(
            transfer(
                f,
                vec![trow("A", NodeStatus::Running, None, None, None, 99)],
            ),
            100,
            3,
        ))
    };
    assert_ne!(
        running(0),
        running(2),
        "a running transfer without a total animates"
    );
    let arriving = |f, age| {
        text(&render(
            transfer(f, vec![trow("A", NodeStatus::Ok, None, None, None, age)]),
            100,
            3,
        ))
    };
    assert_ne!(
        arriving(0, 0),
        arriving(0, 4),
        "a row that just arrived is still moving"
    );
    assert_eq!(arriving(0, 40), arriving(7, 40), "a settled row is still");
}

#[test]
fn transfer_clips_in_tiny_areas() {
    let _ = render(
        transfer(
            1,
            vec![trow("A", NodeStatus::Running, Some(1), Some(2), None, 0)],
        ),
        8,
        2,
    );
    let _ = render(transfer(1, vec![]), 0, 0);
}

#[test]
fn transfer_fits_80_columns_by_ellipsizing_long_names_and_keeps_the_bar_text() {
    let w = transfer(
        0,
        vec![TransferRow {
            label: "onlyoffice/ONLYOFFICE_JWT_SECRET_VERY_LONG".into(),
            from: "kv/opkit/prod/onlyoffice-document-server".into(),
            to: "kv/opkit/preprod/onlyoffice-document-server".into(),
            status: NodeStatus::Running,
            done: Some(120 * 1024 * 1024),
            total: Some(480 * 1024 * 1024),
            detail: None,
            age: 99,
        }],
    );
    let t = text(&render(w, 80, 3));
    let row = t.lines().nth(1).unwrap();
    assert!(
        row.contains('…'),
        "long names are cut with an ellipsis: {row}"
    );
    assert!(row.contains("▒") || row.contains('█'), "{row}");
    assert!(
        row.contains("120.0 MB / 480.0 MB"),
        "the bar text must not be pushed out: {row}"
    );
    assert!(row.chars().count() <= 80, "the row fits the width: {row}");
    assert!(t.lines().next().unwrap().contains("PREPROD"), "{t}");
}
