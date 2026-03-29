use meridian_core::agent::{AgentRecord, AgentState};
use meridian_core::id::{AgentId, CheckpointVersion, ObjectiveId};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, StatefulWidget, Widget},
};
use std::collections::HashMap;

use crate::layout::focused_border_style;

fn state_color(state: &AgentState) -> Color {
    match state {
        AgentState::Active => Color::Green,
        AgentState::Starting | AgentState::Restoring => Color::Yellow,
        AgentState::Draining | AgentState::Paused => Color::Cyan,
        AgentState::Exited | AgentState::Completed => Color::Gray,
        AgentState::Failed => Color::Red,
    }
}

#[derive(Debug, Clone)]
pub struct AgentTreeNode {
    pub id: AgentId,
    pub state: AgentState,
    pub objective_label: String,
    pub tokens_used: Option<u64>,
    pub tokens_remaining: Option<u64>,
    pub checkpoint_version: Option<CheckpointVersion>,
    pub hitl_pending: bool,
}

#[derive(Debug, Clone)]
pub struct TreeNode<T> {
    pub data: T,
    pub children: Vec<TreeNode<T>>,
    pub expanded: bool,
}

#[derive(Debug, Clone)]
pub struct FlatTreeItem<T> {
    pub data: T,
    pub depth: usize,
    pub is_expanded: bool,
    pub has_children: bool,
}

#[derive(Debug)]
pub struct TreePanelState<T> {
    pub items: Vec<FlatTreeItem<T>>,
    pub cursor: usize,
    pub scroll_offset: usize,
}

impl<T> Default for TreePanelState<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            cursor: 0,
            scroll_offset: 0,
        }
    }
}

impl<T> TreePanelState<T> {
    pub fn clamp_cursor(&mut self) {
        if self.items.is_empty() {
            self.cursor = 0;
        } else {
            self.cursor = self.cursor.min(self.items.len() - 1);
        }
    }

    pub fn move_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if !self.items.is_empty() {
            self.cursor = (self.cursor + 1).min(self.items.len() - 1);
        }
    }

    pub fn selected(&self) -> Option<&FlatTreeItem<T>> {
        self.items.get(self.cursor)
    }
}

pub fn flatten_tree<T: Clone>(root: &TreeNode<T>) -> Vec<FlatTreeItem<T>> {
    let mut out = Vec::new();
    flatten_recursive(root, 0, &mut out);
    out
}

fn flatten_recursive<T: Clone>(node: &TreeNode<T>, depth: usize, out: &mut Vec<FlatTreeItem<T>>) {
    out.push(FlatTreeItem {
        data: node.data.clone(),
        depth,
        is_expanded: node.expanded,
        has_children: !node.children.is_empty(),
    });
    if node.expanded {
        for child in &node.children {
            flatten_recursive(child, depth + 1, out);
        }
    }
}

pub fn build_agent_trees(
    records: &[AgentRecord],
    resolve_label: &dyn Fn(ObjectiveId) -> String,
) -> Vec<TreeNode<AgentTreeNode>> {
    let mut nodes: HashMap<AgentId, TreeNode<AgentTreeNode>> = HashMap::new();
    let mut child_map: HashMap<AgentId, Vec<AgentId>> = HashMap::new();
    let mut roots = Vec::new();

    for rec in records {
        let tree_node = TreeNode {
            data: AgentTreeNode {
                id: rec.id,
                state: rec.state,
                objective_label: resolve_label(rec.objective_id),
                tokens_used: None,
                tokens_remaining: None,
                checkpoint_version: rec.checkpoint_version,
                hitl_pending: false,
            },
            children: Vec::new(),
            expanded: true,
        };
        nodes.insert(rec.id, tree_node);

        if let Some(parent) = rec.spawned_by {
            child_map.entry(parent).or_default().push(rec.id);
        } else {
            roots.push(rec.id);
        }
    }

    fn attach_children(
        id: AgentId,
        nodes: &mut HashMap<AgentId, TreeNode<AgentTreeNode>>,
        child_map: &HashMap<AgentId, Vec<AgentId>>,
    ) -> Option<TreeNode<AgentTreeNode>> {
        let children_ids = child_map.get(&id).cloned().unwrap_or_default();
        let children: Vec<TreeNode<AgentTreeNode>> = children_ids
            .into_iter()
            .filter_map(|cid| attach_children(cid, nodes, child_map))
            .collect();

        nodes.remove(&id).map(|mut node| {
            node.children = children;
            node
        })
    }

    roots
        .into_iter()
        .filter_map(|id| attach_children(id, &mut nodes, &child_map))
        .collect()
}

pub type AgentTreeState = TreePanelState<AgentTreeNode>;

pub struct AgentTreeWidget {
    pub focused: bool,
}

impl StatefulWidget for AgentTreeWidget {
    type State = AgentTreeState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let block = Block::default()
            .title(" Agents ")
            .borders(Borders::ALL)
            .border_style(focused_border_style(self.focused));

        let inner = block.inner(area);
        block.render(area, buf);

        let visible_height = inner.height as usize;

        if state.cursor < state.scroll_offset {
            state.scroll_offset = state.cursor;
        } else if state.cursor >= state.scroll_offset + visible_height {
            state.scroll_offset = state.cursor + 1 - visible_height;
        }

        let lines: Vec<Line<'_>> = state
            .items
            .iter()
            .enumerate()
            .skip(state.scroll_offset)
            .take(visible_height)
            .map(|(i, item)| {
                let indent = "  ".repeat(item.depth);
                let arrow = if item.has_children {
                    if item.is_expanded {
                        "v "
                    } else {
                        "> "
                    }
                } else {
                    "  "
                };
                let hitl_mark = if item.data.hitl_pending { "[?] " } else { "" };
                let color = state_color(&item.data.state);
                let text = format!(
                    "{indent}{arrow}{hitl_mark}{} ({})",
                    item.data.id, item.data.state
                );

                let style = if i == state.cursor {
                    Style::default().fg(color).add_modifier(Modifier::REVERSED)
                } else {
                    Style::default().fg(color)
                };

                Line::from(Span::styled(text, style))
            })
            .collect();

        if lines.is_empty() {
            let placeholder =
                Line::styled("  No agents running", Style::default().fg(Color::DarkGray));
            Paragraph::new(vec![placeholder]).render(inner, buf);
        } else {
            Paragraph::new(lines).render(inner, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use meridian_core::agent::AgentState;
    use meridian_core::id::{AgentId, ObjectiveId};
    use std::path::PathBuf;

    fn make_node(id: AgentId, state: AgentState, hitl: bool) -> AgentTreeNode {
        AgentTreeNode {
            id,
            state,
            objective_label: "test".into(),
            tokens_used: None,
            tokens_remaining: None,
            checkpoint_version: None,
            hitl_pending: hitl,
        }
    }

    #[test]
    fn build_agent_tree_from_records() {
        let parent_id = AgentId::new();
        let child_id = AgentId::new();

        let records = vec![
            AgentRecord {
                id: parent_id,
                state: AgentState::Active,
                directory: PathBuf::from("/tmp"),
                objective_id: ObjectiveId::new(),
                checkpoint_version: None,
                spawned_by: None,
                injected_message: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            AgentRecord {
                id: child_id,
                state: AgentState::Starting,
                directory: PathBuf::from("/tmp"),
                objective_id: ObjectiveId::new(),
                checkpoint_version: None,
                spawned_by: Some(parent_id),
                injected_message: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        ];

        let trees = build_agent_trees(&records, &|_| "obj".to_string());
        assert_eq!(trees.len(), 1);
        assert_eq!(trees[0].data.id, parent_id);
        assert_eq!(trees[0].children.len(), 1);
        assert_eq!(trees[0].children[0].data.id, child_id);
    }

    #[test]
    fn flatten_tree_produces_correct_depths() {
        let root = TreeNode {
            data: make_node(AgentId::new(), AgentState::Active, false),
            children: vec![TreeNode {
                data: make_node(AgentId::new(), AgentState::Starting, false),
                children: vec![],
                expanded: true,
            }],
            expanded: true,
        };

        let flat = flatten_tree(&root);
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].depth, 0);
        assert_eq!(flat[1].depth, 1);
    }

    #[test]
    fn flatten_collapsed_hides_children() {
        let root = TreeNode {
            data: make_node(AgentId::new(), AgentState::Active, false),
            children: vec![TreeNode {
                data: make_node(AgentId::new(), AgentState::Starting, false),
                children: vec![],
                expanded: true,
            }],
            expanded: false,
        };

        let flat = flatten_tree(&root);
        assert_eq!(flat.len(), 1);
    }
}
