mod banner;

use anyhow::Result;
use oc_core::permissions::Decision;
use oc_core::{Agent, Chat, Config, Event, InteractivePermission, ModelInfo, Tool};
use oc_mcp::{McpClient, McpTool};
use oc_provider::{AnthropicProvider, OpenAiCompat};
use oc_tools::{self, TaskTool};
use std::io::{BufRead, IsTerminal, Write};
use std::sync::Arc;

const SYSTEM_TMPL: &str = "You are openrust, a coding agent running on-device in {CWD}. \
     Use tools to explore, read, edit files and run commands. Be concise. \
     For larger self-contained work, use the task tool to spawn a subagent. \
     You have a persistent knowledge-graph memory (memory_* tools): record useful facts \
     about the user, projects and decisions, and search them at the start of new sessions.";

const PLAN_SUFFIX: &str = " You are in PLAN MODE: research the codebase and propose a concrete \
     implementation plan. Read-only tools only — do not modify anything. End with the plan as a \
     numbered list.";

fn make_chat(config: &Config, model_override: Option<&str>) -> Result<Arc<dyn Chat>> {
    let mut model = model_override
        .map(String::from)
        .unwrap_or_else(|| config.model.clone());
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok();
    let is_anthropic = anthropic_key.is_some() || model.starts_with("anthropic/");

    if is_anthropic {
        let key = anthropic_key
            .or_else(|| config.api_key.clone())
            .ok_or_else(|| anyhow::anyhow!("anthropic provider needs ANTHROPIC_API_KEY"))?;
        if model.starts_with("anthropic/") {
            model = model.trim_start_matches("anthropic/").to_string();
        }
        if model.is_empty() || model == "openai/gpt-4o-mini" {
            model = "claude-sonnet-4-5".into();
        }
        let base = std::env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| "https://api.anthropic.com".into());
        Ok(Arc::new(AnthropicProvider::new(base, key, model)))
    } else {
        Ok(Arc::new(OpenAiCompat::new(config.base_url.clone(), config.api_key.clone(), model)))
    }
}

fn build_tools(
    config: &Config,
    cwd: &std::path::Path,
    chat: &Arc<dyn Chat>,
) -> Result<(Vec<Box<dyn Tool>>, Vec<Arc<McpClient>>)> {
    let system = SYSTEM_TMPL.replace("{CWD}", &cwd.display().to_string());
    let mut tools: Vec<Box<dyn Tool>> = vec![
        Box::new(oc_tools::Bash::new(cwd.to_path_buf())),
        Box::new(oc_tools::Read),
        Box::new(oc_tools::Write),
        Box::new(oc_tools::Edit),
        Box::new(oc_tools::Glob),
        Box::new(TaskTool::new(Arc::clone(chat), cwd.to_path_buf(), system)),
    ];
    tools.extend(oc_memory::tools(oc_core::session::data_dir()?.join("memory.json"))?);
    let mut mcp: Vec<Arc<McpClient>> = Vec::new();
    for (i, srv) in config.mcp_servers.iter().enumerate() {
        let name = srv.name.clone().unwrap_or_else(|| format!("server{i}"));
        match McpClient::connect(&srv.command, &name) {
            Ok(c) => {
                let c = Arc::new(c);
                match c.list_tools() {
                    Ok(infos) => {
                        eprintln!("[mcp] {name}: {} tools", infos.len());
                        for info in infos {
                            tools.push(Box::new(McpTool::new(Arc::clone(&c), info)));
                        }
                        mcp.push(c);
                    }
                    Err(e) => {
                        eprintln!("[mcp] {name}: list failed: {e}");
                        c.shutdown();
                    }
                }
            }
            Err(e) => eprintln!("[mcp] {name}: connect failed: {e}"),
        }
    }
    Ok((tools, mcp))
}

fn main() -> Result<()> {
    let config = Config::load()?;
    let cwd = std::env::current_dir()?;
    let chat = make_chat(&config, None)?;

    if std::io::stdin().is_terminal() {
        // interactive: homepage, then TUI session
        if !oc_tui::home(env!("CARGO_PKG_VERSION"))? {
            return Ok(());
        }
        let (tools, _mcp) = build_tools(&config, &cwd, &chat)?;
        let mut agent = Agent::new(Arc::clone(&chat), SYSTEM_TMPL.replace("{CWD}", &cwd.display().to_string()), tools);
        let saved = oc_core::session::load_history(&cwd);
        if !saved.is_empty() {
            agent.history = saved;
        }
        oc_tui::session::run(agent, cwd, config.model.clone(), env!("CARGO_PKG_VERSION").to_string())?;
        return Ok(());
    }

    // non-interactive: banner + line REPL
    banner::print_banner();
    repl(config, cwd, chat)
}

fn repl(config: Config, cwd: std::path::PathBuf, chat: Arc<dyn Chat>) -> Result<()> {
    let mut chat = chat;
    let (tools, mcp) = build_tools(&config, &cwd, &chat)?;
    let mut agent = Agent::new(
        Arc::clone(&chat),
        SYSTEM_TMPL.replace("{CWD}", &cwd.display().to_string()),
        tools,
    );

    // non-interactive permissions: read/glob always; everything else per config mode
    let mode = config.permission_mode.clone();
    agent.permissions = Some(Arc::new(InteractivePermission {
        ask: Box::new(move |tool: &str, _args: &str| -> Decision {
            if oc_core::permissions::is_safe(tool) {
                return Decision::Allow;
            }
            if mode == "allow" {
                Decision::Allow
            } else {
                eprintln!(
                    "[perm] denied {tool} (non-interactive; set \"permissions\": {{\"mode\": \"allow\"}} in config to auto-allow)"
                );
                Decision::Deny
            }
        }),
    }));

    let saved = oc_core::session::load_history(&cwd);
    if !saved.is_empty() {
        let n = saved.len();
        agent.history = saved;
        eprintln!("[session] resumed {n} messages");
    }

    if config.anonymous() {
        eprintln!("anonymous mode | model: {} (free tier)", config.model);
    } else {
        eprintln!("keyed mode | model: {} | {}", config.model, config.base_url);
    }
    eprintln!("streaming agent (bash/read/write/edit/glob + MCP) | /models /model <id> /mcp /reset /exit\n");

    let stdin = std::io::stdin();
    loop {
        eprint!("> ");
        std::io::stderr().flush()?;

        let mut line = String::new();
        stdin.lock().read_line(&mut line)?;
        let line = line.trim();

        if line.is_empty() {
            continue;
        }
        match line {
            "/exit" | "/quit" => break,
            "/reset" => {
                agent.reset();
                oc_core::session::clear_history(&cwd);
                eprintln!("session cleared\n");
            }
            "/mcp" => {
                for c in &mcp {
                    eprintln!("[mcp] {} (server {})", c.server_name, c.server_name);
                }
                if mcp.is_empty() {
                    eprintln!("[mcp] none connected");
                }
                eprintln!();
            }
            "/models" => match ModelInfo::list_free(&config) {
                Ok(models) if models.is_empty() => eprintln!("no free models listed\n"),
                Ok(models) => {
                    eprintln!("free models:");
                    for m in models {
                        eprintln!("  {:30} {} — {}", m.id, m.name, m.description);
                    }
                    eprintln!();
                }
                Err(e) => eprintln!("error: {e}\n"),
            },
            _ if line.starts_with("/model ") => {
                let id = line.trim_start_matches("/model ").trim();
                if id.is_empty() {
                    eprintln!("usage: /model <id>\n");
                } else {
                    chat = make_chat(&config, Some(id))?;
                    agent.swap_chat(Arc::clone(&chat));
                    eprintln!("model set to {id}\n");
                }
            }
            "/plan" => {
                agent.plan_mode = !agent.plan_mode;
                if agent.plan_mode {
                    agent.set_system(format!("{}{PLAN_SUFFIX}", SYSTEM_TMPL.replace("{CWD}", &cwd.display().to_string())));
                    eprintln!("plan mode ON (read-only tools; agent proposes a plan)\n");
                } else {
                    agent.set_system(SYSTEM_TMPL.replace("{CWD}", &cwd.display().to_string()));
                    eprintln!("plan mode OFF\n");
                }
            }
            _ => {
                let mut streamed = false;
                let result = agent.run(line, |ev| match ev {
                    Event::Delta(d) => {
                        streamed = true;
                        print!("{d}");
                        std::io::stdout().flush().ok();
                    }
                    Event::ToolCall { name, args } => {
                        if streamed {
                            println!();
                            streamed = false;
                        }
                        eprintln!("[tool] {name} {args}");
                    }
                    Event::ToolResult { name, ok } => {
                        eprintln!("[tool] {name} -> {}", if ok { "ok" } else { "failed" });
                    }
                    Event::Reply(_) => {}
                });
                match result {
                    Ok(reply) => {
                        if streamed {
                            println!();
                        } else {
                            println!("{reply}\n");
                        }
                        oc_core::session::save_history(&cwd, &agent.history);
                    }
                    Err(e) => eprintln!("error: {e}\n"),
                }
            }
        }
    }

    for c in &mcp {
        c.shutdown();
    }
    Ok(())
}
