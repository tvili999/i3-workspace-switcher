use std::convert::TryInto;
use std::env;

use std::collections::HashMap;

use i3ipc::I3Connection;
use i3ipc::reply::{Node, Workspaces, Workspace, NodeType};

#[derive(Debug)]
struct WorkspaceInfo {
    id: u16,
    name: String,
    active_windows: u8
}

#[derive(Debug)]
struct WorkspaceRing {
    workspaces: Vec<WorkspaceInfo>,
    current_workspace: u16,
    next_id: u16
}

#[derive(Debug)]
struct Output {
    workspace_rings: HashMap<u16, WorkspaceRing>,
    current_workspace_ring: u16
}

#[derive(Debug)]
struct ProgramState {
    conn: I3Connection,
    outputs: HashMap<String, Output>,
    current_output: String,
    next_ring_id: u16
}

#[derive(Copy, Clone, Debug)]
enum Direction {
    Left, Right, Up, Down    
}

#[derive(Copy, Clone, Debug)]
enum Command {
    Switch(Direction), Move(Direction)
}

fn get_workspace_from_tree<'a>(ws: &'_ Workspace, root: &'a Node) -> Option<&'a Node> {
    for node in root.nodes.iter() {
        if let Some(display_name) = &node.name {
            if display_name != &ws.output {
                continue;
            }
            for ws_node in node.nodes.iter() {
                for ws_content_node in ws_node.nodes.iter() {
                    if ws_content_node.nodetype != NodeType::Workspace {
                        continue
                    }

                    if let Some(ws_name) = &ws_content_node.name {
                        if ws_name == &ws.name {
                            return Some(ws_content_node);
                        }
                    }
                }
            }
        }
    }

    None
}

fn count_windows_from_tree(tree: &Node) -> u8 {
    let mut res = 0;

    for node in tree.nodes.iter() {
        if let Some(_) = node.window {
            res = res + 1;
        }
        res += count_windows_from_tree(&node);
    }

    res
}

fn get_program_state() -> ProgramState {
    let mut conn = I3Connection::connect().unwrap();

    let tree = conn.get_tree().unwrap();
    let workspaces = conn.get_workspaces().unwrap();

    let mut next_ring_id = 0;
    let mut outputs: HashMap<String, Output> = HashMap::new();
    let mut current_output = String::from("");
    for workspace in workspaces.workspaces.iter() {
        let output_name = String::from(&workspace.output);
        if !outputs.contains_key(&output_name) {
            outputs.insert(output_name.clone(), Output {
                workspace_rings: HashMap::new(),
                current_workspace_ring: 0
            });
        }

        if let Some(output) = outputs.get_mut(&output_name) {
            let ring_id: u16 = (workspace.num / 100).try_into().unwrap();
            if next_ring_id <= ring_id {
                next_ring_id = ring_id + 1;
            }
            
            if !output.workspace_rings.contains_key(&ring_id) {
                output.workspace_rings.insert(ring_id, WorkspaceRing {
                    current_workspace: workspace.num.try_into().unwrap(), next_id: 0, workspaces: vec![]
                });
            }

            if let Some(workspace_ring) = output.workspace_rings.get_mut(&ring_id) {
                let node = get_workspace_from_tree(&workspace, &tree).unwrap();

                let workspace_id: u16 = workspace.num.try_into().unwrap();
                let workspace_info = WorkspaceInfo {
                    id: workspace_id,
                    name: String::from(&workspace.name),
                    active_windows: count_windows_from_tree(&node)
                };

                if workspace.visible || workspace_info.name.contains("_act") {
                    workspace_ring.current_workspace = workspace_ring.workspaces.len().try_into().unwrap();
                }

                if workspace_ring.next_id <= workspace_id {
                    workspace_ring.next_id = workspace_id + 1;
                }

                workspace_ring.workspaces.push(workspace_info);
            }

            if workspace.focused {
                output.current_workspace_ring = ring_id;
                current_output = output_name.clone();
            }
        }
    }

    let state = ProgramState { conn, outputs, current_output, next_ring_id };
    println!("{:#?}", state);
    state
}

fn switch(direction: Direction, state: &mut ProgramState) {
    let output = state.outputs.get(&state.current_output).unwrap();
    let workspace_ring = output.workspace_rings.get(&output.current_workspace_ring).unwrap();
    let workspace_idx : usize = workspace_ring.current_workspace.try_into().unwrap();

    if let Direction::Left = direction {
        if workspace_idx == 0  {
            state.conn.run_command(format!("workspace {}", workspace_ring.next_id).as_str()).unwrap();
        }
        else {
            let new_workspace = workspace_ring.workspaces.get(workspace_idx - 1).unwrap();
            state.conn.run_command(format!("workspace {}", new_workspace.id).as_str()).unwrap();
        }
    }
    if let Direction::Right = direction {
        if workspace_idx == workspace_ring.workspaces.len() - 1 {
            let current_workspace = workspace_ring.workspaces.get(workspace_idx).unwrap();
            if current_workspace.active_windows == 0 {
                let new_workspace = workspace_ring.workspaces.get(0).unwrap();
                state.conn.run_command(format!("workspace {}", new_workspace.id).as_str()).unwrap();
            }
            else {
                state.conn.run_command(format!("workspace {}", workspace_ring.next_id).as_str()).unwrap();
            }
        }
        else {
            let new_workspace = workspace_ring.workspaces.get(workspace_idx + 1).unwrap();
            state.conn.run_command(format!("workspace {}", new_workspace.id).as_str()).unwrap();
        }
    }

    if let Direction::Down = direction {
        let current_workspace = workspace_ring.workspaces.get(workspace_idx).unwrap();

        let mut workspace_ring_ids: Vec<u16> = output.workspace_rings.keys().map(|x| *x).collect();
        workspace_ring_ids.sort();
        let current_ws_ring_idx: usize = workspace_ring_ids.iter().position(|x| x == &output.current_workspace_ring).unwrap();

        if current_ws_ring_idx == workspace_ring_ids.len() - 1 {
            if current_workspace.active_windows == 0 {
                let ws_ring = output.workspace_rings.get(workspace_ring_ids.get(0).unwrap()).unwrap();
                state.conn.run_command(format!("workspace {}", ws_ring.current_workspace).as_str()).unwrap();
            }
            else {
                state.conn.run_command(format!("workspace {}", state.next_ring_id * 100).as_str()).unwrap();
            }
        }
        else {
            let ws_ring = output.workspace_rings.get(workspace_ring_ids.get(current_ws_ring_idx + 1).unwrap()).unwrap();
            state.conn.run_command(format!("workspace {}", ws_ring.current_workspace).as_str()).unwrap();
        }
    }

    if let Direction::Up = direction {
        let mut workspace_ring_ids: Vec<u16> = output.workspace_rings.keys().map(|x| *x).collect();
        workspace_ring_ids.sort();
        let current_ws_ring_idx: usize = workspace_ring_ids.iter().position(|x| x == &output.current_workspace_ring).unwrap();

        if current_ws_ring_idx == 0 {
            state.conn.run_command(format!("workspace {}", state.next_ring_id * 100).as_str()).unwrap();
        }
        else {
            let ws_ring = output.workspace_rings.get(workspace_ring_ids.get(current_ws_ring_idx - 1).unwrap()).unwrap();
            state.conn.run_command(format!("workspace {}", ws_ring.current_workspace).as_str()).unwrap();
        }
    }
}

fn move_window(direction: Direction, state: &mut ProgramState) {
    let output = state.outputs.get(&state.current_output).unwrap();
    let workspace_ring = output.workspace_rings.get(&output.current_workspace_ring).unwrap();
    let workspace_idx: usize = workspace_ring.current_workspace.try_into().unwrap();

    if let Direction::Left = direction {
        if workspace_idx == 0  {
            state.conn.run_command(format!("move container to workspace {}", workspace_ring.next_id).as_str()).unwrap();
            state.conn.run_command(format!("workspace {}", workspace_ring.next_id).as_str()).unwrap();
        }
        else {
            let new_workspace = workspace_ring.workspaces.get(workspace_idx - 1).unwrap();
            state.conn.run_command(format!("move container to workspace {}", new_workspace.id).as_str()).unwrap();
            state.conn.run_command(format!("workspace {}", new_workspace.id).as_str()).unwrap();
        }
    }
    if let Direction::Right = direction {
        if workspace_idx == workspace_ring.workspaces.len() - 1 {
            let current_workspace = workspace_ring.workspaces.get(workspace_idx).unwrap();
            if current_workspace.active_windows <= 1 {
                let new_workspace = workspace_ring.workspaces.get(0).unwrap();
                state.conn.run_command(format!("move container to workspace {}", new_workspace.id).as_str()).unwrap();
                state.conn.run_command(format!("workspace {}", new_workspace.id).as_str()).unwrap();
            }
            else {
                state.conn.run_command(format!("move container to workspace {}", workspace_ring.next_id).as_str()).unwrap();
                state.conn.run_command(format!("workspace {}", workspace_ring.next_id).as_str()).unwrap();
            }
        }
        else {
            let new_workspace = workspace_ring.workspaces.get(workspace_idx + 1).unwrap();
            state.conn.run_command(format!("move container to workspace {}", new_workspace.id).as_str()).unwrap();
            state.conn.run_command(format!("workspace {}", new_workspace.id).as_str()).unwrap();
        }
    }

    if let Direction::Down = direction {
        let current_workspace = workspace_ring.workspaces.get(workspace_idx).unwrap();

        let mut workspace_ring_ids: Vec<u16> = output.workspace_rings.keys().map(|x| *x).collect();
        workspace_ring_ids.sort();
        let current_ws_ring_idx: usize = workspace_ring_ids.iter().position(|x| x == &output.current_workspace_ring).unwrap();

        if current_ws_ring_idx == workspace_ring_ids.len() - 1 {
            if current_workspace.active_windows <= 1 {
                let ws_ring = output.workspace_rings.get(workspace_ring_ids.get(0).unwrap()).unwrap();
                state.conn.run_command(format!("move container to workspace {}", ws_ring.current_workspace).as_str()).unwrap();
                state.conn.run_command(format!("workspace {}", ws_ring.current_workspace).as_str()).unwrap();
            }
            else {
                state.conn.run_command(format!("move container to workspace {}", state.next_ring_id * 100).as_str()).unwrap();
                state.conn.run_command(format!("workspace {}", state.next_ring_id * 100).as_str()).unwrap();
            }
        }
        else {
            let ws_ring = output.workspace_rings.get(workspace_ring_ids.get(current_ws_ring_idx + 1).unwrap()).unwrap();
            state.conn.run_command(format!("move container to workspace {}", ws_ring.current_workspace).as_str()).unwrap();
            state.conn.run_command(format!("workspace {}", ws_ring.current_workspace).as_str()).unwrap();
        }
    }

    if let Direction::Up = direction {
        let mut workspace_ring_ids: Vec<u16> = output.workspace_rings.keys().map(|x| *x).collect();
        workspace_ring_ids.sort();
        let current_ws_ring_idx: usize = workspace_ring_ids.iter().position(|x| x == &output.current_workspace_ring).unwrap();

        if current_ws_ring_idx == 0 {
            state.conn.run_command(format!("move container to workspace {}", state.next_ring_id * 100).as_str()).unwrap();
            state.conn.run_command(format!("workspace {}", state.next_ring_id * 100).as_str()).unwrap();
        }
        else {
            let ws_ring = output.workspace_rings.get(workspace_ring_ids.get(current_ws_ring_idx - 1).unwrap()).unwrap();
            state.conn.run_command(format!("move container to workspace {}", ws_ring.current_workspace).as_str()).unwrap();
            state.conn.run_command(format!("workspace {}", ws_ring.current_workspace).as_str()).unwrap();
        }
    }
}

fn parse_command(args: &Vec<String>) -> Result<Command, ()> {
    if args.len() != 3 {
        return Result::Err(());
    }

    let cmd = args.get(1).unwrap();
    let dir = args.get(2).unwrap();

    let dir = {
        if dir == "left" {
            Ok(Direction::Left)
        }
        else if dir == "right" {
            Ok(Direction::Right)
        }
        else if dir == "up" {
            Ok(Direction::Up)
        }
        else if dir == "down" {
            Ok(Direction::Down)
        }
        else {
            Err(())
        }
    };

    if let Ok(dir) = dir {
        if cmd == "move" {
            return Ok(Command::Move(dir))
        }
        else if cmd == "switch" {
            return Ok(Command::Switch(dir))
        }
    }

    Result::Err(())
}

fn main() {
    if let Ok(command) = parse_command(&env::args().collect()) {
        let mut state = get_program_state();

        match command {
            Command::Switch(dir) => {
                switch(dir, &mut state);
            },
            Command::Move(dir) => {
                move_window(dir, &mut state);
            }
        }
    }
    else {
        println!("Wrong arguments");
    }
}
