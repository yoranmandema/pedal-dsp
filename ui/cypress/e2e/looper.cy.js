// Looper transport panel: banner state, progress, five momentary triggers,
// four knobs and two toggle chips.
//
// NOTE: the engine card (t_8f27e62e) is not merged yet, so this spec drives the
// real `time.looper` spec through the seeded rig. The fake engine (fake-engine.mjs)
// translates the UI's momentary trig_* gates into the engine's looper state
// machine (docs/looper.md §4) and broadcasts `event: looper` (~12.5 Hz) while
// Playing/Overdubbing.

describe("looper transport panel", () => {
  beforeEach(() => {
    cy.boot();
    // Looper in Channel 2 (ch2 blocks: 0 drive, 1 voices, 2 looper).
    cy.selectBlock(1, 2);
  });

  it("renders the panel with the correct classes and labels", () => {
    cy.get(".editor-name").should("contain", "Looper");
    cy.get(".looper-banner-state").should("have.text", "STOPPED");
    cy.get(".looper-triggers .looper-trigger").should("have.length", 5);
    cy.get(".looper-trigger").eq(0).should("contain", "Record");
    cy.get(".looper-trigger").eq(1).should("contain", "Play");
    cy.get(".looper-trigger").eq(2).should("contain", "Overdub");
    cy.get(".looper-trigger").eq(3).should("contain", "Stop");
    cy.get(".looper-trigger").eq(4).should("contain", "Clear");
    cy.get(".looper-knobs .knob-unit").should("have.length", 4);
    cy.get(".looper-knobs .knob-name").eq(0).should("have.text", "Loop length");
    cy.get(".looper-knobs .knob-name").eq(1).should("have.text", "Feedback");
    cy.get(".looper-knobs .knob-name").eq(2).should("have.text", "Mix");
    cy.get(".looper-knobs .knob-name").eq(3).should("have.text", "Level");
    cy.get(".looper-chips .choice-unit").should("have.length", 2);
    cy.get(".looper-chips .choice-unit").eq(0).find(".ctrlName").should("contain", "Half speed");
    cy.get(".looper-chips .choice-unit").eq(1).find(".ctrlName").should("contain", "Reverse");
  });

  it("starts in STOPPED with the Record gate up", () => {
    cy.get(".looper-trigger").contains("Record").should("not.have.attr", "aria-pressed", "true");
    cy.get(".looper-banner-state").should("have.text", "STOPPED");
    cy.get(".looper-progress-fill").should("have.css", "width", "0px");
  });

  it("drives the banner and trigger gate with Record", () => {
    cy.get(".looper-trigger").contains("Record").as("record");
    cy.get(".looper-trigger").contains("Record").pointer("pointerdown", 50, 50);
    cy.wait(300);
    cy.get("@record").should("have.attr", "aria-pressed", "true");
    cy.get(".looper-banner-state").should("have.text", "RECORDING");
    cy.get(".looper-trigger").contains("Record").pointer("pointerup", 50, 50);
    cy.wait(300);
    cy.get("@record").should("have.attr", "aria-pressed", "false");
    cy.get(".looper-banner-state").should("have.text", "STOPPED");
  });

  it("records, plays and stops a take", () => {
    // Record: gate 1.0 -> engine transitions to RECORDING.
    cy.get(".looper-trigger").contains("Record").pointer("pointerdown", 50, 50);
    cy.wait(400);
    cy.get(".looper-banner-state").should("have.text", "RECORDING");
    cy.get(".looper-trigger").contains("Record").pointer("pointerup", 50, 50);
    cy.wait(300);

    // Play: gate 1.0 -> PLAYING; the progress fill begins to advance.
    cy.get(".looper-trigger").contains("Play").pointer("pointerdown", 50, 50);
    cy.wait(400);
    cy.get(".looper-banner-state").should("have.text", "PLAYING");
    cy.get(".looper-progress-fill").should("have.css", "width", "0%");
    // The fake engine advances looper_frac ~0.005 every 80 ms while Playing.
    cy.wait(2000);
    cy.get(".looper-progress-fill").should("have.css", "width", ">", "0%");

    // Stop: gate 1.0 -> STOPPED.
    cy.get(".looper-trigger").contains("Stop").pointer("pointerdown", 50, 50);
    cy.wait(400);
    cy.get(".looper-banner-state").should("have.text", "STOPPED");
  });

  it("sends setParam on knob drag and chip toggles", () => {
    // Time knob drag changes the value away from the default.
    cy.get(".looper-knobs .knob-unit").eq(0).find(".knob")
      .trigger("pointerdown", { pointerId: 1, button: 0, clientY: 300 })
      .trigger("pointermove", { pointerId: 1, clientY: 100 })
      .trigger("pointerup", { pointerId: 1 });
    cy.get(".looper-knobs .knob-unit").eq(0).find(".knob-value").should("not.have.text", "1000.00");

    // Half chip: press ON, then press OFF.
    cy.get(".looper-chips .choice-unit").contains("On").as("halfChipOn");
    cy.get("@halfChipOn").click();
    cy.get("@halfChipOn").should("have.attr", "aria-checked", "true");
    cy.get(".looper-chips .choice-unit").contains("Off").as("halfChipOff");
    cy.get("@halfChipOff").click();
    cy.get("@halfChipOff").should("have.attr", "aria-checked", "true");
  });

  it("operates the Play trigger with the keyboard", () => {
    cy.get(".looper-trigger").contains("Play").focus();
    cy.get(".looper-trigger").contains("Play").key("keydown", "Enter");
    cy.wait(300);
    cy.get(".looper-banner-state").should("have.text", "PLAYING");
    cy.get(".looper-trigger").contains("Play").key("keyup", "Enter");
    cy.wait(300);
    cy.get(".looper-banner-state").should("have.text", "STOPPED");
  });
});
