//! Interactive TUI for the knowledge graph.
//!
//! Three panes:
//! 1. **Top**: dashboard banner (counts, lean status, open questions).
//! 2. **Left**: list of nodes (filterable by type / confidence / lean status).
//! 3. **Right**: detail view of the selected node + its outgoing neighbors.
//!
//! Keys:
//! - `↑` / `↓` / `j` / `k` — move selection
//! - `t`           — cycle node-type filter
//! - `f`           — toggle "low-confidence only" filter
//! - `l`           — toggle "Lean-failed only" filter
//! - `r`           — refresh from DB
//! - `q` / `Esc`   — quit

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Terminal;

use dt_knowledge::{KnowledgeNode, KnowledgeRepository, NeighborDirection, NodeType};

use crate::dashboard::{Dashboard, DashboardStats};

const TYPE_FILTERS: &[Option<NodeType>] = &[
    None,
    Some(NodeType::Note),
    Some(NodeType::Hypothesis),
    Some(NodeType::Insight),
    Some(NodeType::Reflection),
    Some(NodeType::Theorem),
    Some(NodeType::Evidence),
    Some(NodeType::CognitivePattern),
    Some(NodeType::MetaQuestion),
];

struct App<'a> {
    repo: &'a KnowledgeRepository,
    nodes: Vec<KnowledgeNode>,
    state: ListState,
    type_filter_idx: usize,
    only_low_confidence: bool,
    only_lean_failed: bool,
    stats: DashboardStats,
    err: Option<String>,
}

impl<'a> App<'a> {
    fn new(repo: &'a KnowledgeRepository) -> Result<Self> {
        let stats = Dashboard::new(repo).compute(10_000)?;
        let mut app = Self {
            repo,
            nodes: Vec::new(),
            state: ListState::default(),
            type_filter_idx: 0,
            only_low_confidence: false,
            only_lean_failed: false,
            stats,
            err: None,
        };
        app.reload()?;
        Ok(app)
    }

    fn reload(&mut self) -> Result<()> {
        let filter = TYPE_FILTERS[self.type_filter_idx].as_ref();
        let mut nodes = self.repo.list(filter, 500)?;
        if self.only_low_confidence {
            nodes.retain(|n| n.metadata.dt_confidence.unwrap_or(1.0) < 0.5);
        }
        if self.only_lean_failed {
            nodes.retain(|n| {
                n.lean
                    .as_ref()
                    .map(|l| l.lean_proof_status == dt_knowledge::LeanProofStatus::Failed)
                    .unwrap_or(false)
            });
        }
        self.nodes = nodes;
        if !self.nodes.is_empty() {
            self.state.select(Some(0));
        } else {
            self.state.select(None);
        }
        self.stats = Dashboard::new(self.repo).compute(10_000)?;
        Ok(())
    }

    fn next(&mut self) {
        if self.nodes.is_empty() {
            return;
        }
        let i = self
            .state
            .selected()
            .map(|i| (i + 1) % self.nodes.len())
            .unwrap_or(0);
        self.state.select(Some(i));
    }

    fn prev(&mut self) {
        if self.nodes.is_empty() {
            return;
        }
        let i = self
            .state
            .selected()
            .map(|i| (i + self.nodes.len() - 1) % self.nodes.len())
            .unwrap_or(0);
        self.state.select(Some(i));
    }

    fn cycle_type_filter(&mut self) -> Result<()> {
        self.type_filter_idx = (self.type_filter_idx + 1) % TYPE_FILTERS.len();
        self.reload()
    }

    fn current_filter_label(&self) -> String {
        match &TYPE_FILTERS[self.type_filter_idx] {
            None => "all".to_string(),
            Some(t) => t.as_str().to_string(),
        }
    }
}

/// Run the TUI. Blocks until the user quits.
pub fn run(repo: &KnowledgeRepository) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, repo);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn run_loop<B: Backend>(terminal: &mut Terminal<B>, repo: &KnowledgeRepository) -> Result<()> {
    let mut app = App::new(repo)?;
    loop {
        terminal.draw(|f| ui(f, &mut app))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('j') | KeyCode::Down => app.next(),
                KeyCode::Char('k') | KeyCode::Up => app.prev(),
                KeyCode::Char('t') => {
                    if let Err(e) = app.cycle_type_filter() {
                        app.err = Some(e.to_string());
                    }
                }
                KeyCode::Char('f') => {
                    app.only_low_confidence = !app.only_low_confidence;
                    if let Err(e) = app.reload() {
                        app.err = Some(e.to_string());
                    }
                }
                KeyCode::Char('l') => {
                    app.only_lean_failed = !app.only_lean_failed;
                    if let Err(e) = app.reload() {
                        app.err = Some(e.to_string());
                    }
                }
                KeyCode::Char('r') => {
                    if let Err(e) = app.reload() {
                        app.err = Some(e.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn ui(f: &mut ratatui::Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Length(5), Constraint::Min(0)].as_ref())
        .split(f.size());

    render_dashboard(f, chunks[0], &app.stats);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)].as_ref())
        .split(chunks[1]);

    render_list(f, body[0], app);
    render_detail(f, body[1], app);
}

fn render_dashboard(f: &mut ratatui::Frame, rect: Rect, stats: &DashboardStats) {
    let mc_pct = if stats.total_nodes > 0 {
        (stats.total_meta_cognitive as f64 / stats.total_nodes as f64) * 100.0
    } else {
        0.0
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "DT Knowledge Graph",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "  ·  nodes={}  meta-cog={} ({:.0}%)  open_questions={}",
                stats.total_nodes, stats.total_meta_cognitive, mc_pct, stats.open_questions
            )),
        ]),
        Line::from(format!(
            "Lean: verified={} pending={} failed={}",
            stats.lean_verified, stats.lean_pending, stats.lean_failed
        )),
        Line::from(format!(
            "Confidence buckets [0,.2)/[.2,.4)/[.4,.6)/[.6,.8)/[.8,1] = {:?}  none={}",
            stats.confidence.buckets, stats.confidence.none_count
        )),
    ];
    let p = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" Dashboard "));
    f.render_widget(p, rect);
}

fn render_list(f: &mut ratatui::Frame, rect: Rect, app: &mut App) {
    let items: Vec<ListItem> = app
        .nodes
        .iter()
        .map(|n| {
            let badge = badge_for(n);
            ListItem::new(format!("{} {} {}", n.node_type.as_str(), badge, n.content.title))
        })
        .collect();
    let title = format!(
        " Nodes  filter:type={}  low-conf:{}  lean-failed:{}  (t/f/l to toggle, q to quit) ",
        app.current_filter_label(),
        if app.only_low_confidence { "ON" } else { "off" },
        if app.only_lean_failed { "ON" } else { "off" },
    );
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("→ ");
    f.render_stateful_widget(list, rect, &mut app.state);
}

fn render_detail(f: &mut ratatui::Frame, rect: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(" Detail ");
    let Some(idx) = app.state.selected() else {
        f.render_widget(Paragraph::new("(no selection)").block(block), rect);
        return;
    };
    let Some(n) = app.nodes.get(idx) else {
        f.render_widget(Paragraph::new("(out of range)").block(block), rect);
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            format!("[{}] ", n.node_type.as_str()),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(
            n.content.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(format!("id: {}", n.node_id)));
    if let Some(c) = n.metadata.dt_confidence {
        lines.push(Line::from(format!("confidence: {:.2}", c)));
    }
    if let Some(lean) = &n.lean {
        lines.push(Line::from(format!(
            "lean: {} (verifier={}, hash={})",
            lean.lean_proof_status.as_str(),
            lean.verifier_version.clone().unwrap_or_default(),
            lean.lean_theorem_hash.clone().unwrap_or_default()
        )));
        if let Some(err) = &lean.last_error {
            lines.push(Line::from(format!("lean_error: {}", err)));
        }
    }
    if let Some(mc) = &n.meta_cognition {
        lines.push(Line::from(format!(
            "certainty: {} | derivation_depth: {}",
            mc.certainty_type.as_str(),
            mc.derivation_depth
        )));
        if !mc.assumptions.is_empty() {
            lines.push(Line::from("assumptions:"));
            for a in &mc.assumptions {
                lines.push(Line::from(format!("  • {}", a)));
            }
        }
        if !mc.counter_arguments.is_empty() {
            lines.push(Line::from("counter-arguments:"));
            for c in &mc.counter_arguments {
                lines.push(Line::from(format!("  • {}", c)));
            }
        }
        if !mc.open_questions.is_empty() {
            lines.push(Line::from("open questions:"));
            for q in &mc.open_questions {
                lines.push(Line::from(format!("  ? {}", q)));
            }
        }
        if !mc.thinking_trace.is_empty() {
            lines.push(Line::from("thinking trace:"));
            for s in &mc.thinking_trace {
                lines.push(Line::from(format!("  → {}", s.thought)));
            }
        }
    }

    if !n.content.body.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "body",
            Style::default().fg(Color::Magenta),
        )));
        for body_line in n.content.body.lines() {
            lines.push(Line::from(body_line.to_string()));
        }
    }

    if let Ok(edges) = app
        .repo
        .neighbors(&n.node_id, NeighborDirection::Outgoing, None, 32)
    {
        if !edges.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "outgoing edges",
                Style::default().fg(Color::Green),
            )));
            for e in edges {
                lines.push(Line::from(format!(
                    "  --[{}]--> {}",
                    e.relation.as_str(),
                    e.target_id
                )));
            }
        }
    }

    if let Some(err) = &app.err {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("error: {}", err),
            Style::default().fg(Color::Red),
        )));
    }

    let p = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    f.render_widget(p, rect);
}

fn badge_for(n: &KnowledgeNode) -> String {
    let conf = n
        .metadata
        .dt_confidence
        .map(|c| format!("c={:.2}", c))
        .unwrap_or_else(|| "c=?".into());
    let lean = match &n.lean {
        None => String::new(),
        Some(l) => format!(" ⊢{}", l.lean_proof_status.as_str()),
    };
    format!("[{}{}]", conf, lean)
}
