//! Agent Planner Basics: how a natural-language request becomes a safe, recorded map operation.
//! Every request goes through the planner's stages, one at a time:
//!
//!   1 request -> 2 proposal (workflow DSL v2) -> 3 validation -> 4 deterministic execution -> 5 provenance
//!
//! Five lessons show what each stage guarantees: a plan that runs, the same plan giving the same
//! result, proposals that never run, confirmation bound to one exact plan, and a provenance record
//! that shows any change made to it. The lessons run offline with prepared proposals; your own
//! requests need an LLM (GEOKERNEL_LLM_PROVIDER). Rust port of the Qt example.
//!
//! Needs geokernel with the agent API (newer than 1.5.40): `window.agent`, `Runtime::propose`,
//! `GisAgent::verify_provenance`.
use geokernel::{Runtime, ViewerWindow};
use serde_json::{json, Value};
use std::{
    error::Error,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const TITLE: &str = "Agent Planner Basics";
const LESSON: i32 = 1; // 1..=5
const TYPED: i32 = 10;
const PLAN: i32 = 11;
const RUN: i32 = 12;
const UNDO: i32 = 13;
const PLANNER: i32 = 14;
const LESSON_TITLES: [&str; 5] = [
    "1. A plan that reads data and changes the view",
    "2. Same plan, same result",
    "3. Proposals that never run",
    "4. Changing data needs confirmation",
    "5. A record that shows any change",
];

// Bottom to top; each new layer becomes index 0.
const LAYERS: [(&str, &str); 4] = [
    ("parks.geojson", r##"{"fillColor":"#A1D99B","fillOpacity":255,"lineColor":"#41AB5D","lineWidth":1.0}"##),
    ("streets.geojson", r##"{"lineColor":"#9E9E9E","lineWidth":3.0}"##),
    ("buildings.geojson", r##"{"fillColor":"#E7D8C3","fillOpacity":255,"lineColor":"#A08B70","lineWidth":0.8}"##),
    ("schools.geojson", r##"{"pointColor":"#2171B5","fillColor":"#2171B5","lineColor":"#08306B","pointSize":12.0,"lineWidth":1.2}"##),
];

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn short(value: &str) -> String {
    if value.is_empty() { "(none)".into() } else { format!("{}…", value.chars().take(12).collect::<String>()) }
}

fn status(result: &Value) -> &str {
    match text(&result["status"]) {
        "confirmation-required" => "confirmation required",
        other => other,
    }
}

fn steps(result: &Value) -> i64 {
    result["completedSteps"].as_i64().unwrap_or(0)
}

/// A proposal as the planner receives it: JSON text, or the prose of a model that ignored the contract.
fn proposal_text(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string())
}

/// The five lessons on the window's agent session; writes each stage to the log panel.
struct Lessons<'r> {
    runtime: &'r Runtime,
    data: Value,
    smoke: bool,
    runs: Vec<i32>, // completed steps per executed workflow, for undo
    pending: Option<(String, Value)>,
    planning: Option<Receiver<std::result::Result<String, String>>>,
}

impl<'r> Lessons<'r> {
    // -------------------------------------------------------------- output

    fn log(&self, window: &mut ViewerWindow, line: &str) -> Result<()> {
        window.append_log(line)?;
        if self.smoke {
            println!("{line}");
        }
        Ok(())
    }

    fn heading(&self, window: &mut ViewerWindow, lesson: usize, title: &str) -> Result<()> {
        self.log(window, "")?;
        let prefix = if lesson > 0 { format!("Lesson {lesson} · ") } else { String::new() };
        self.log(window, &format!("━━ {prefix}{title}"))
    }

    fn line(&self, window: &mut ViewerWindow, stage: &str, value: &str) -> Result<()> {
        self.log(window, &format!("{stage}  {value}"))
    }

    fn detail(&self, window: &mut ViewerWindow, value: &str) -> Result<()> {
        self.log(window, &format!("      {value}"))
    }

    // -------------------------------------------------------------- stages

    fn execute(&mut self, window: &mut ViewerWindow, proposal: &str, confirmed: Option<&str>) -> Result<Value> {
        let result = window.agent(self.runtime)?.run(proposal, confirmed)?;
        if steps(&result) > 0 {
            self.runs.push(steps(&result) as i32);
        }
        Ok(result)
    }

    fn reset(&mut self, window: &mut ViewerWindow) -> Result<()> {
        while let Some(count) = self.runs.pop() {
            window.agent(self.runtime)?.undo(count)?;
        }
        Ok(())
    }

    fn records(&self, window: &mut ViewerWindow) -> Result<Vec<Value>> {
        Ok(window.agent(self.runtime)?.provenance()?["records"].as_array().cloned().unwrap_or_default())
    }

    fn stage_proposal(&self, window: &mut ViewerWindow, source: &str, proposal: &str) -> Result<()> {
        self.line(window, "② Proposal", source)?;
        match serde_json::from_str::<Value>(proposal) {
            Ok(Value::Object(mut plan)) => {
                let steps = plan.remove("steps").and_then(|s| s.as_array().cloned()).unwrap_or_default();
                let head = Value::Object(plan).to_string();
                self.detail(window, &format!("{},\"steps\":[", &head[..head.len() - 1]))?;
                for step in &steps {
                    self.detail(window, &format!("  {step}"))?;
                }
                self.detail(window, "]}")
            }
            _ => self.detail(window, &proposal.chars().take(160).collect::<String>()),
        }
    }

    fn stage_validation(&self, window: &mut ViewerWindow, proposal: &str) -> Result<Value> {
        let evaluation = window.agent(self.runtime)?.evaluate(proposal)?;
        self.line(window, "③ Validation", &format!("{} · {} — {}", status(&evaluation), text(&evaluation["code"]), text(&evaluation["message"])))?;
        let plan_id = text(&evaluation["planId"]).to_owned();
        for summary_line in text(&evaluation["summary"]).lines().filter(|l| !l.is_empty()) {
            let shown = if plan_id.is_empty() { summary_line.to_owned() } else { summary_line.replace(&plan_id, &short(&plan_id)) };
            self.detail(window, &shown)?;
        }
        Ok(evaluation)
    }

    fn stage_execution(&self, window: &mut ViewerWindow, run: &Value) -> Result<()> {
        self.line(window, "④ Execution", &format!("{} · {} steps · {}", status(run), steps(run), text(&run["message"])))?;
        for report_line in text(&run["report"]).lines().filter(|l| !l.is_empty()) {
            self.detail(window, report_line)?;
        }
        Ok(())
    }

    fn stage_provenance(&self, window: &mut ViewerWindow, run: &Value) -> Result<()> {
        let records = self.records(window)?;
        let Some(record) = records.last().filter(|_| !text(&run["provenanceHash"]).is_empty()) else {
            return self.line(window, "⑤ Provenance", "nothing ran, nothing recorded");
        };
        self.line(window, "⑤ Provenance", &format!("record #{} · hash {} ← previous {}", record["sequence"], short(text(&record["hash"])), short(text(&record["previousHash"]))))?;
        self.detail(window, &format!("plan    {}", short(text(&record["validationAudit"]["planId"]))))?;
        if let Some(inputs) = record["inputSha256"].as_object() {
            for (name, value) in inputs {
                self.detail(window, &format!("input   {name:<18} {}", short(text(value))))?;
            }
        }
        let count = record["stepOutputs"].as_array().map_or(0, Vec::len);
        self.detail(window, &format!("output  {} ({count} step outputs)", short(text(&record["outputSha256"]))))
    }

    // -------------------------------------------------------------- lessons

    fn run_lesson(&mut self, window: &mut ViewerWindow, index: usize) -> Result<bool> {
        self.pending = None;
        let ok = match index {
            0 => self.basic(window)?,
            1 => self.determinism(window)?,
            2 => self.rejected(window)?,
            3 => self.confirmation(window)?,
            _ => self.tamper(window)?,
        };
        window.viewer().refresh_layers()?;
        window.set_status_text(&format!("Lesson {} {}", index + 1, if ok { "done" } else { "did not behave as described" }))?;
        self.refresh(window)?;
        Ok(ok)
    }

    fn basic(&mut self, window: &mut ViewerWindow) -> Result<bool> {
        let lesson = self.data["basic"].clone();
        self.heading(window, 1, text(&lesson["title"]))?;
        self.reset(window)?;
        let proposal = proposal_text(&lesson["proposal"]);
        self.line(window, "① Request", &format!("“{}”", text(&lesson["request"])))?;
        self.stage_proposal(window, "prepared, exactly as a model returns it", &proposal)?;
        let evaluation = self.stage_validation(window, &proposal)?;
        let run = self.execute(window, &proposal, None)?;
        self.stage_execution(window, &run)?;
        self.stage_provenance(window, &run)?;
        Ok(evaluation["status"] == "validated" && run["status"] == "executed")
    }

    fn determinism(&mut self, window: &mut ViewerWindow) -> Result<bool> {
        let proposal = proposal_text(&self.data["basic"]["proposal"]);
        self.heading(window, 2, "Same plan, same result")?;
        self.log(window, "The plan from lesson 1 runs, is undone, and runs again on the same data.")?;
        self.reset(window)?;
        let first = self.execute(window, &proposal, None)?;
        let a = self.records(window)?.last().cloned().ok_or("no provenance record")?;
        self.line(window, "④ First run", &format!("{} · {} steps · record #{}", status(&first), steps(&first), a["sequence"]))?;
        self.reset(window)?;
        let second = self.execute(window, &proposal, None)?;
        let b = self.records(window)?.last().cloned().ok_or("no provenance record")?;
        self.line(window, "④ Second run", &format!("{} · {} steps · record #{}", status(&second), steps(&second), b["sequence"]))?;
        let same_inputs = a["inputSha256"] == b["inputSha256"];
        let same_steps = a["stepOutputs"] == b["stepOutputs"];
        let same_output = !text(&a["outputSha256"]).is_empty() && a["outputSha256"] == b["outputSha256"];
        self.line(window, "⑤ Compare", &format!(
            "input hashes {} · output {} = {} {}",
            if same_inputs { "identical" } else { "DIFFERENT" },
            short(text(&a["outputSha256"])), short(text(&b["outputSha256"])),
            if same_output && same_steps { "✓ identical, step by step" } else { "✗ different" }
        ))?;
        for step in b["stepOutputs"].as_array().into_iter().flatten() {
            self.detail(window, &format!("{:<14} {}", text(&step["stepId"]), short(text(&step["outputSha256"]))))?;
        }
        self.log(window, &format!("Records #{} and #{} differ only in their execution id, time and chain position; \
            anyone can replay a record and compare the output hash.", a["sequence"], b["sequence"]))?;
        Ok(same_inputs && same_steps && same_output)
    }

    fn rejected(&mut self, window: &mut ViewerWindow) -> Result<bool> {
        self.heading(window, 3, "Proposals that never run")?;
        self.log(window, "A careless or manipulated model can return anything. Each proposal below goes to the same planner; none reaches the map.")?;
        self.reset(window)?;
        let before = self.records(window)?.len();
        let mut all_stopped = true;
        let items = self.data["rejected"].as_array().cloned().unwrap_or_default();
        for (index, item) in items.iter().enumerate() {
            let proposal = proposal_text(&item["proposal"]);
            self.log(window, "")?;
            self.log(window, &format!("{}. {}", index + 1, text(&item["title"])))?;
            self.stage_proposal(window, "bad", &proposal)?;
            let evaluation = window.agent(self.runtime)?.evaluate(&proposal)?;
            let run = self.execute(window, &proposal, None)?;
            let stopped = steps(&run) == 0 && run["status"] != "executed" && evaluation["status"] != "validated";
            all_stopped &= stopped;
            self.line(window, "③ Validation", &format!("{} · {} — {}", status(&evaluation), text(&evaluation["code"]), text(&evaluation["message"])))?;
            self.line(window, "④ Execution", if stopped { "not started: nothing ran" } else { "✗ it ran" })?;
            self.detail(window, text(&item["why"]))?;
        }
        let after = self.records(window)?.len();
        self.line(window, "⑤ Provenance", &format!("{before} records before, {after} after: refused proposals leave nothing to record"))?;
        Ok(all_stopped && before == after && !items.is_empty())
    }

    fn confirmation(&mut self, window: &mut ViewerWindow) -> Result<bool> {
        let lesson = self.data["confirm"].clone();
        self.heading(window, 4, text(&lesson["title"]))?;
        self.reset(window)?;
        let proposal = proposal_text(&lesson["proposal"]);
        self.line(window, "① Request", &format!("“{}”", text(&lesson["request"])))?;
        self.stage_proposal(window, "prepared", &proposal)?;
        let evaluation = self.stage_validation(window, &proposal)?;
        let plan_id = text(&evaluation["planId"]).to_owned();

        let unconfirmed = self.execute(window, &proposal, None)?;
        self.line(window, "④ Run, not confirmed", &format!("{} · {} steps ran", status(&unconfirmed), steps(&unconfirmed)))?;

        let mut changed = lesson["proposal"].clone();
        for step in changed["steps"].as_array_mut().into_iter().flatten() {
            if step["id"] == lesson["changedStep"] {
                step["parameters"]["values"] = lesson["changedValues"].clone();
            }
        }
        let changed_text = changed.to_string();
        let changed_id = text(&window.agent(self.runtime)?.evaluate(&changed_text)?["planId"]).to_owned();
        let swapped = self.execute(window, &changed_text, Some(&plan_id))?;
        self.line(window, "④ Swapped plan", &format!(
            "{}: plan {} with the confirmation of {} → {} · {} steps ran",
            text(&lesson["changedTitle"]), short(&changed_id), short(&plan_id), status(&swapped), steps(&swapped)
        ))?;

        let confirmed = self.execute(window, &proposal, Some(&plan_id))?;
        self.line(window, "④ Confirmed run", &format!("the user confirms plan {}", short(&plan_id)))?;
        self.stage_execution(window, &confirmed)?;
        self.stage_provenance(window, &confirmed)?;
        Ok(evaluation["status"] == "confirmation-required"
            && unconfirmed["status"] == "confirmation-required" && steps(&unconfirmed) == 0
            && changed_id != plan_id
            && swapped["status"] == "confirmation-required" && steps(&swapped) == 0
            && confirmed["status"] == "executed")
    }

    fn tamper(&mut self, window: &mut ViewerWindow) -> Result<bool> {
        self.heading(window, 5, "A record that shows any change")?;
        if self.records(window)?.is_empty() {
            self.log(window, "No workflow has run yet; running lesson 1 first.")?;
            self.basic(window)?;
        }
        let exported = window.agent(self.runtime)?.provenance()?;
        let records = exported["records"].as_array().cloned().unwrap_or_default();
        self.line(window, "⑤ Export", &format!("{} records, {} bytes of JSON", records.len(), exported.to_string().len()))?;
        for record in &records {
            self.detail(window, &format!("#{} {:<12} {} ← {}", record["sequence"], text(&record["executionId"]), short(text(&record["hash"])), short(text(&record["previousHash"]))))?;
        }
        let intact = window.agent(self.runtime)?.verify_provenance(Some(&exported))?;
        let verified = intact["verified"] == true;
        self.line(window, "⑤ Verify", &if verified { "✓ every record matches its hash and the chain".to_owned() } else { format!("✗ {}", text(&intact["message"])) })?;

        // Somebody edits one output hash in the saved file.
        let mut edited = exported.clone();
        let hash = text(&edited["records"][0]["outputSha256"]).to_owned();
        let flipped = format!("{}{}", &hash[..hash.len() - 1], if hash.ends_with('0') { "1" } else { "0" });
        edited["records"][0]["outputSha256"] = json!(flipped);
        let broken = window.agent(self.runtime)?.verify_provenance(Some(&edited))?;
        let detected = broken["verified"] == false && broken["brokenAtSequence"] == 1;
        self.line(window, "⑤ Edited copy", &format!("record #1 output hash changed in one character → {}",
            if detected { "✗ record 1 no longer matches its hash: the change is detected" } else { "not detected" }))?;
        self.log(window, "Each record's hash covers the normalized plan, the validation decision, the input and output hashes and the previous record's hash.")?;
        Ok(verified && detected)
    }

    // -------------------------------------------------------------- your own request

    fn plan_own(&mut self, window: &mut ViewerWindow, request: &str) -> Result<()> {
        let request = request.trim().to_owned();
        if request.is_empty() || self.planning.is_some() {
            return Ok(());
        }
        self.pending = None;
        self.heading(window, 0, "Your request")?;
        self.reset(window)?;
        self.line(window, "① Request", &format!("“{request}”"))?;
        // The model call can take a while: run it on a worker thread with its own runtime
        // (GeoKernelGisLlm_Propose keeps its result per thread) and keep pumping events.
        let grounding = format!("{}\n{}", window.agent(self.runtime)?.grounding()?, text(&self.data["groundingNotes"]));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let outcome = (|| -> geokernel::Result<String> {
                let runtime = unsafe { Runtime::from_env()? };
                runtime.propose(None, &request, &grounding)
            })();
            let _ = tx.send(outcome.map_err(|error| error.to_string()));
        });
        self.planning = Some(rx);
        window.set_status_text("Asking the model for a plan…")?;
        self.refresh(window)
    }

    fn poll_planner(&mut self, window: &mut ViewerWindow) -> Result<()> {
        let Some(outcome) = self.planning.as_ref().and_then(|rx| rx.try_recv().ok()) else { return Ok(()) };
        self.planning = None;
        window.set_status_text("")?;
        match outcome {
            Err(error) => self.line(window, "② Proposal", &format!("the model returned no plan: {error}"))?,
            Ok(proposal) => {
                self.stage_proposal(window, "from the model", &proposal)?;
                let evaluation = self.stage_validation(window, &proposal)?;
                if evaluation["status"] == "validated" || evaluation["status"] == "confirmation-required" {
                    let confirm = evaluation["needsConfirmation"].as_array().map_or(false, |a| !a.is_empty());
                    self.log(window, if confirm { "The plan changes data. Clicking Run (confirms this plan) confirms this exact plan." } else { "Click Run to execute the plan." })?;
                    self.pending = Some((proposal, evaluation));
                }
            }
        }
        self.refresh(window)
    }

    fn run_own(&mut self, window: &mut ViewerWindow) -> Result<()> {
        let Some((proposal, evaluation)) = self.pending.take() else { return Ok(()) };
        // Clicking Run is the user's confirmation of this exact plan.
        let confirmed = evaluation["needsConfirmation"].as_array().filter(|a| !a.is_empty()).and(evaluation["planId"].as_str());
        let run = self.execute(window, &proposal, confirmed)?;
        self.stage_execution(window, &run)?;
        self.stage_provenance(window, &run)?;
        window.viewer().refresh_layers()?;
        self.refresh(window)
    }

    fn refresh(&self, window: &mut ViewerWindow) -> Result<()> {
        let ready = self.planning.is_none();
        for index in 0..LESSON_TITLES.len() as i32 {
            window.set_control_enabled(LESSON + index, ready)?;
        }
        let llm = std::env::var("GEOKERNEL_LLM_PROVIDER").map_or(false, |p| !p.trim().is_empty());
        window.set_control_enabled(PLAN, ready && llm)?;
        window.set_control_enabled(RUN, ready && self.pending.is_some())?;
        window.set_control_enabled(UNDO, ready)?;
        Ok(())
    }
}

fn run(runtime: &Runtime, window: &mut ViewerWindow, data: &Path, smoke: bool) -> Result<bool> {
    let lessons_data: Value = serde_json::from_str(&std::fs::read_to_string(data.join("planner-lessons.json"))?)?;
    let provider = std::env::var("GEOKERNEL_LLM_PROVIDER").ok().map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty());
    let planner = match &provider {
        Some(p) => {
            let model = std::env::var("GEOKERNEL_LLM_MODEL").ok().filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| if p == "anthropic" { "claude-opus-5-5".into() } else { "model not set".into() });
            format!("{p} ({model})")
        }
        None => "lessons run offline".into(),
    };
    let mut typed = if provider.is_some() { "Show the parks with more than 50 trees in dark green".to_owned() } else { String::new() };
    let mut controls: Vec<Value> = LESSON_TITLES.iter().enumerate()
        .map(|(index, title)| json!({"id": LESSON + index as i32, "type": "button", "text": title}))
        .collect();
    controls.extend([
        json!({"id": TYPED, "type": "text", "label": "Your own request", "value": typed,
               "placeholder": if provider.is_some() { "" } else { "Set GEOKERNEL_LLM_PROVIDER to plan your own requests" }}),
        json!({"id": PLAN, "type": "button", "text": "Plan"}),
        json!({"id": RUN, "type": "button", "text": "Run (confirms this plan)"}),
        json!({"id": UNDO, "type": "button", "text": "Undo all"}),
        json!({"id": PLANNER, "type": "text", "label": "Your requests", "value": planner, "readOnly": true}),
    ]);
    let events = window.add_control_panel(&json!({"title": "Planner, stage by stage", "width": 360, "controls": controls}).to_string())?;
    window.add_log_panel("Planner stages")?;
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

    let mut lessons = Lessons { runtime, data: lessons_data, smoke, runs: Vec::new(), pending: None, planning: None };
    lessons.log(window, "Each lesson sends a request through the planner's five stages: ① request → ② proposal → \
        ③ validation → ④ execution → ⑤ provenance. A lesson starts from the original map; its result stays on the map.")?;
    lessons.refresh(window)?;

    if smoke {
        let mut results = Vec::new();
        for index in 0..LESSON_TITLES.len() {
            results.push(lessons.run_lesson(window, index)?);
            window.process_events();
        }
        for (index, ok) in results.iter().enumerate() {
            println!("smoke | lesson {}: {}", index + 1, if *ok { "ok" } else { "FAILED" });
        }
        return Ok(results.iter().all(|ok| *ok));
    }

    while window.is_visible()? {
        window.process_events();
        while let Ok(event) = events.try_recv() {
            match event.id {
                id if (LESSON..LESSON + LESSON_TITLES.len() as i32).contains(&id) => {
                    lessons.run_lesson(window, (id - LESSON) as usize)?;
                }
                TYPED => typed = event.text.clone(),
                PLAN => lessons.plan_own(window, &typed)?,
                RUN => lessons.run_own(window)?,
                UNDO => {
                    lessons.reset(window)?;
                    window.viewer().refresh_layers()?;
                    window.set_status_text("Every lesson result was undone")?;
                }
                _ => {}
            }
        }
        lessons.poll_planner(window)?;
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(true)
}

fn main() -> Result<()> {
    let smoke = std::env::args().any(|argument| argument == "--smoke"); // run the five lessons, print them, quit
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data");
    // This process owns the main thread and loads the trusted runtime named by GEOKERNEL_BIN.
    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, TITLE, 1440, 900)? };
        run(&runtime, &mut window, &data, smoke)
    })();
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    let passed = outcome?;
    shutdown?;
    if !passed {
        std::process::exit(2);
    }
    Ok(())
}
