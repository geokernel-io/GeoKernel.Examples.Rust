//! Agent Map Assistant: natural-language map requests as validated, previewed and recorded
//! workflows. A planner (an LLM, or the offline demo with prepared plans) proposes a workflow
//! DSL v2 plan; GeoKernel validates it, the plan is shown before anything runs, steps that
//! change data need the user's confirmation of that exact plan, and every executed workflow is
//! undoable and recorded with hash-chained provenance. Rust port of the Qt example.
//!
//! Needs geokernel with the agent API (newer than 1.5.40): `window.agent`, `Runtime::propose`.
use geokernel::{Runtime, ViewerWindow};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    error::Error,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const TITLE: &str = "Agent Map Assistant";
const REQUESTS: i32 = 1;
const ASK: i32 = 2;
const TYPED: i32 = 3;
const SEND: i32 = 4;
const RUN: i32 = 5;
const CONFIRM: i32 = 6;
const DISCARD: i32 = 7;
const UNDO: i32 = 8;
const PROVENANCE: i32 = 9;
const FULL_EXTENT: i32 = 10;
const PLANNER: i32 = 11;

// Bottom to top; each new layer becomes index 0.
const LAYERS: [(&str, &str); 4] = [
    ("parks.geojson", r##"{"fillColor":"#A1D99B","fillOpacity":255,"lineColor":"#41AB5D","lineWidth":1.0}"##),
    ("streets.geojson", r##"{"lineColor":"#9E9E9E","lineWidth":3.0}"##),
    ("buildings.geojson", r##"{"fillColor":"#E7D8C3","fillOpacity":255,"lineColor":"#A08B70","lineWidth":0.8}"##),
    ("schools.geojson", r##"{"pointColor":"#2171B5","fillColor":"#2171B5","lineColor":"#08306B","pointSize":12.0,"lineWidth":1.2}"##),
];

struct Demo {
    notes: String,
    requests: Vec<(String, String)>, // request, proposal
}

fn load_demo(data: &Path) -> Result<Demo> {
    let root: Value = serde_json::from_str(&std::fs::read_to_string(data.join("agent-demo.json"))?)?;
    let requests = root["requests"]
        .as_array()
        .ok_or("agent-demo.json has no requests")?
        .iter()
        .map(|item| (item["request"].as_str().unwrap_or_default().to_owned(), item["proposal"].to_string()))
        .collect();
    Ok(Demo { notes: root["groundingNotes"].as_str().unwrap_or_default().to_owned(), requests })
}

fn steps(count: i64) -> String {
    if count == 1 { "1 step".into() } else { format!("{count} steps") }
}

fn short(value: &Value, length: usize) -> String {
    value.as_str().unwrap_or("-").chars().take(length).collect()
}

/// The conversation state; every call touches the window on the GUI thread.
struct Assistant<'r> {
    runtime: &'r Runtime,
    demo: Demo,
    prepared: HashMap<String, String>,
    provider: Option<String>,
    selected: String,
    typed: String,
    pending: Option<(String, Value)>, // proposal, evaluation
    workflows: Vec<i32>,
    planning: Option<Receiver<std::result::Result<String, String>>>,
}

impl<'r> Assistant<'r> {
    fn say(&self, window: &mut ViewerWindow, who: &str, text: &str) -> Result<()> {
        window.append_log(&format!("{who}: {text}"))?;
        Ok(())
    }

    fn needs_confirmation(&self) -> bool {
        self.pending.as_ref().map_or(false, |(_, plan)| plan["needsConfirmation"].as_array().map_or(false, |a| !a.is_empty()))
    }

    fn refresh(&self, window: &mut ViewerWindow) -> Result<()> {
        let ready = self.planning.is_none();
        let confirm = self.needs_confirmation();
        window.set_control_enabled(ASK, ready)?;
        window.set_control_enabled(SEND, ready)?;
        window.set_control_enabled(RUN, ready && self.pending.is_some() && !confirm)?;
        window.set_control_enabled(CONFIRM, ready && confirm)?;
        window.set_control_enabled(DISCARD, ready && self.pending.is_some())?;
        window.set_control_enabled(UNDO, ready && !self.workflows.is_empty())?;
        Ok(())
    }

    fn submit(&mut self, window: &mut ViewerWindow, request: &str) -> Result<()> {
        let request = request.trim().to_owned();
        if request.is_empty() || self.planning.is_some() {
            return Ok(());
        }
        self.discard(window, false)?;
        self.say(window, "You", &request)?;
        if self.provider.is_none() {
            return match self.prepared.get(&request.to_lowercase()).cloned() {
                Some(proposal) => self.preview(window, &proposal),
                None => {
                    self.say(window, "Planner", "No plan was proposed: offline demo mode only has plans for the sample requests. \
                        Set GEOKERNEL_LLM_PROVIDER (anthropic, openai or ollama) and its API key to plan your own requests.")?;
                    self.refresh(window)
                }
            };
        }
        // The model call can take a while: run it on a worker thread with its own runtime
        // (GeoKernelGisLlm_Propose keeps its result per thread) and keep pumping events.
        let grounding = format!("{}\n{}", window.agent(self.runtime)?.grounding()?, self.demo.notes);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = (|| -> geokernel::Result<String> {
                let runtime = unsafe { Runtime::from_env()? };
                runtime.propose(None, &request, &grounding)
            })();
            let _ = tx.send(outcome.map_err(|error| error.to_string()));
        });
        self.planning = Some(rx);
        window.set_status_text("Planning...")?;
        self.refresh(window)
    }

    fn poll_planner(&mut self, window: &mut ViewerWindow) -> Result<()> {
        let Some(outcome) = self.planning.as_ref().and_then(|rx| rx.try_recv().ok()) else { return Ok(()) };
        self.planning = None;
        window.set_status_text("")?;
        match outcome {
            Ok(proposal) => self.preview(window, &proposal),
            Err(error) => {
                self.say(window, "Planner", &format!("No plan was proposed: {error}"))?;
                self.refresh(window)
            }
        }
    }

    fn preview(&mut self, window: &mut ViewerWindow, proposal: &str) -> Result<()> {
        let plan = window.agent(self.runtime)?.evaluate(proposal)?;
        let status = plan["status"].as_str().unwrap_or_default().to_owned();
        if status != "validated" && status != "confirmation-required" {
            let details: Vec<String> = plan["steps"].as_array().into_iter().flatten()
                .filter(|step| step["message"].as_str().map_or(false, |m| !m.is_empty()))
                .map(|step| format!("{}: {}", step["stepId"].as_str().unwrap_or("-"), step["message"].as_str().unwrap_or_default()))
                .collect();
            let suffix = if details.is_empty() { String::new() } else { format!(" — {}", details.join("; ")) };
            self.say(window, "Planner", &format!("The plan was rejected ({status}): {}{suffix}", plan["message"].as_str().unwrap_or_default()))?;
            return self.refresh(window);
        }
        let id = plan["planId"].as_str().unwrap_or_default();
        let summary = plan["summary"].as_str().unwrap_or_default().replacen(id, &format!("{}…", &id[..id.len().min(16)]), 1);
        let count = summary.lines().count() as i64 - 1;
        let confirm: Vec<&str> = plan["needsConfirmation"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        if confirm.is_empty() {
            self.say(window, "Planner", &format!("Plan ready ({}). Review it and press Run.", steps(count)))?;
            window.set_status_text("Plan validated: review it and click Run")?;
        } else {
            self.say(window, "Planner", &format!("Plan ready. Steps {} change or delete data; review them, then Confirm and run.", confirm.join(", ")))?;
            window.set_status_text("The plan changes data: review it and click Confirm and run")?;
        }
        window.append_log(&summary)?;
        self.pending = Some((proposal.to_owned(), plan));
        self.refresh(window)
    }

    fn run_pending(&mut self, window: &mut ViewerWindow) -> Result<()> {
        let Some((proposal, plan)) = self.pending.take() else { return Ok(()) };
        // Clicking Run / Confirm and run is the user's confirmation of this exact plan.
        let confirmed = plan["needsConfirmation"].as_array().filter(|a| !a.is_empty()).and(plan["planId"].as_str());
        let result = window.agent(self.runtime)?.run(&proposal, confirmed)?;
        let completed = result["completedSteps"].as_i64().unwrap_or(0);
        if completed > 0 {
            self.workflows.push(completed as i32);
        }
        if let Some(report) = result["report"].as_str().filter(|r| !r.is_empty()) {
            self.say(window, "Result", report)?;
        }
        if result["status"] == "executed" {
            self.say(window, "Map", &format!("Done: {} ran. Provenance {}.", steps(completed), short(&result["provenanceHash"], 12)))?;
            window.set_status_text(&format!("Executed {}", short(&result["planId"], 16)))?;
        } else {
            let failed = result["failedStepId"].as_str().filter(|s| !s.is_empty()).unwrap_or("-");
            self.say(window, "Map", &format!("Stopped at step {failed}: {}. {completed} completed steps can be undone.", result["message"].as_str().unwrap_or_default()))?;
        }
        self.refresh(window)
    }

    fn discard(&mut self, window: &mut ViewerWindow, log: bool) -> Result<()> {
        if self.pending.take().is_some() && log {
            self.say(window, "Planner", "Plan discarded; nothing ran.")?;
            window.set_status_text("")?;
        }
        self.refresh(window)
    }

    fn undo_last(&mut self, window: &mut ViewerWindow) -> Result<()> {
        let Some(count) = self.workflows.pop() else { return Ok(()) };
        let result = window.agent(self.runtime)?.undo(count)?;
        let undone: Vec<&str> = result["undoneSteps"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        let mut text = format!("Undone: {}.", undone.join(", "));
        let irreversible: Vec<&str> = result["irreversible"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
        if !irreversible.is_empty() {
            text.push_str(&format!(" Could not undo: {}.", irreversible.join("; ")));
        }
        self.say(window, "Map", &text)?;
        self.refresh(window)
    }

    fn show_provenance(&mut self, window: &mut ViewerWindow) -> Result<()> {
        let records = window.agent(self.runtime)?.provenance()?["records"].as_array().cloned().unwrap_or_default();
        if records.is_empty() {
            return self.say(window, "Provenance", "No workflow has run yet.");
        }
        self.say(window, "Provenance", &format!("{} executed workflow(s), chained by hash:", records.len()))?;
        for record in &records {
            window.append_log(&format!(
                "  #{} {}: {} step outputs, output {}, hash {} <- {}",
                record["sequence"], record["executionId"].as_str().unwrap_or("-"),
                record["stepOutputs"].as_array().map_or(0, Vec::len),
                short(&record["outputSha256"], 12), short(&record["hash"], 12), short(&record["previousHash"], 12)
            ))?;
        }
        Ok(())
    }
}

fn run(runtime: &Runtime, window: &mut ViewerWindow, data: &Path) -> Result<()> {
    let demo = load_demo(data)?;
    let provider = std::env::var("GEOKERNEL_LLM_PROVIDER").ok().map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty());
    let planner = match &provider {
        Some(p) => {
            let model = std::env::var("GEOKERNEL_LLM_MODEL").ok().filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| if p == "anthropic" { "claude-opus-5-5".into() } else { "model not set".into() });
            format!("{p} ({model})")
        }
        None => "offline demo (prepared plans)".into(),
    };
    let options: Vec<&str> = demo.requests.iter().map(|(request, _)| request.as_str()).collect();
    let events = window.add_control_panel(&json!({"title":"Map assistant","width":330,"controls":[
        {"id":REQUESTS,"type":"combo","label":"Try","options":options,"value":options[0]},
        {"id":ASK,"type":"button","text":"Ask"},
        {"id":TYPED,"type":"text","label":"Your request","value":""},
        {"id":SEND,"type":"button","text":"Send"},
        {"id":RUN,"type":"button","text":"Run"},
        {"id":CONFIRM,"type":"button","text":"Confirm and run"},
        {"id":DISCARD,"type":"button","text":"Discard"},
        {"id":UNDO,"type":"button","text":"Undo last"},
        {"id":PROVENANCE,"type":"button","text":"Provenance"},
        {"id":FULL_EXTENT,"type":"button","text":"Full Extent"},
        {"id":PLANNER,"type":"text","label":"Planner","value":planner,"readOnly":true}
    ]}).to_string())?;
    window.add_log_panel("Conversation")?;
    window.show()?;
    window.process_events();
    {
        let mut viewer = window.viewer();
        for (file, style) in LAYERS {
            let path = data.join(file);
            if !viewer.add_layer_file(path.to_str().ok_or("Invalid data path")?)? {
                return Err(format!("{file} could not be loaded").into());
            }
            viewer.set_layer_style_json(0, style)?;
        }
        viewer.refresh_layers()?;
        viewer.full_extent()?;
    }
    window.agent(runtime)?; // fails early when the runtime has no agent API

    let mut assistant = Assistant {
        runtime,
        prepared: demo.requests.iter().map(|(request, proposal)| (request.to_lowercase(), proposal.clone())).collect(),
        selected: demo.requests[0].0.clone(),
        demo,
        provider,
        typed: String::new(),
        pending: None,
        workflows: Vec::new(),
        planning: None,
    };
    assistant.say(window, "Assistant", "Pick a request under Try and click Ask, or type your own and click Send. Plans are shown here before anything runs.")?;
    assistant.refresh(window)?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event) = events.try_recv() {
            match event.id {
                REQUESTS => {
                    assistant.selected = if event.text.is_empty() {
                        assistant.demo.requests.get(event.number as usize).map(|(r, _)| r.clone()).unwrap_or_default()
                    } else {
                        event.text.clone()
                    }
                }
                TYPED => assistant.typed = event.text.clone(),
                ASK => { let request = assistant.selected.clone(); assistant.submit(window, &request)?; }
                SEND => { let request = assistant.typed.clone(); assistant.submit(window, &request)?; }
                RUN | CONFIRM => assistant.run_pending(window)?,
                DISCARD => assistant.discard(window, true)?,
                UNDO => assistant.undo_last(window)?,
                PROVENANCE => assistant.show_provenance(window)?,
                FULL_EXTENT => window.viewer().full_extent()?,
                _ => {}
            }
        }
        assistant.poll_planner(window)?;
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn main() -> Result<()> {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
    // This process owns the main thread and loads the trusted runtime named by GEOKERNEL_BIN.
    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, TITLE, 1360, 820)? };
        run(&runtime, &mut window, &data)
    })();
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;
    Ok(())
}
