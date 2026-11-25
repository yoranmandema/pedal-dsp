// The pitch whammy block editor: six controls (pitch, sweep rate, mode, glide,
// feedback, mix) + footswitch button + expression-pedal slider. Binds to the
// engine's `sweep_gate` param (see docs/design/pitch-whammy.md §5).
//
// Runs against a rig seeded with the whammy block in
// `ui/cypress/support/seed.mjs`.

describe("pitch whammy block editor", () => {
  beforeEach(() => {
    cy.boot();
    // Open the add-block picker on channel 1, filter to the whammy, and add it.
    cy.get("[data-add=\"ch1\"]").click();
    cy.get(".picker-search").type("Pitch Whammy");
    cy.get(".block-list .rack-item").contains("Pitch Whammy").click();
  });

  it("offers the whammy block in the add-block picker under Pitch", () => {
    cy.get("[data-add=\"ch1\"]").click();
    cy.get(".picker-search").type("pitch whammy");
    cy.get(".block-list .rack-item").contains("Pitch Whammy").should("exist");
  });

  it("renders all six controls plus footswitch and expression slider", () => {
    // After adding, the whammy block is the last in channel 1's block array.
    cy.selectBlock(0, 3);
    cy.get(".editor-name").should("contain", "Pitch Whammy");
    // Six knobs + one segmented choice + the footswitch button + the
    // expression slider.
    cy.get(".knob-unit").should("have.length", 6);
    cy.get(".segmented").should("have.length", 1);
    cy.get(".footswitch").should("exist");
    cy.get(".expression-range").should("exist");
    // The editor renders the default values the spec declares.
    cy.get(".knob-value").eq(0).should("contain", "0.00 st");
    cy.get(".knob-value").eq(1).should("contain", "120.00 cents/s");
    cy.get(".segmented .seg").eq(0).should("contain", "Classic");
    cy.get(".knob-value").eq(3).should("contain", "80.00 ms");
    cy.get(".knob-value").eq(4).should("contain", "60.00 %");
    cy.get(".knob-value").eq(5).should("contain", "50.00 %");
  });

  it("toggles the footswitch gate", () => {
    cy.selectBlock(0, 3);
    cy.get(".footswitch").contains("Engage").click();
    cy.get(".footswitch").should("contain", "Disengage");
    cy.get(".footswitch").should("have.attr", "aria-checked", "true");
  });

  it("drives the expression pedal slider", () => {
    cy.selectBlock(0, 3);
    cy.get(".expression-range").invoke("val", 100).trigger("input");
    cy.get(".expression-range").should("have.value", "100");
    cy.get(".footswitch").should("contain", "Disengage");
    cy.get(".expression-range").invoke("val", 0).trigger("input");
    cy.get(".footswitch").should("contain", "Engage");
  });

  it("renders the mode segmented row with the three exact labels", () => {
    cy.selectBlock(0, 3);
    cy.get(".segmented .seg").should("have.length", 3);
    cy.get(".segmented .seg").eq(0).should("contain", "Classic");
    cy.get(".segmented .seg").eq(1).should("contain", "Sweep");
    cy.get(".segmented .seg").eq(2).should("contain", "Harmony");
    cy.get(".segmented .seg").eq(0).should("have.class", "on");
  });

  it("resets every parameter from the editor header", () => {
    cy.selectBlock(0, 3);
    cy.get(".knob-value").eq(0).should("contain", "0.00 st");
    cy.get(".footswitch").contains("Engage").click();
    cy.get(".footswitch").should("contain", "Disengage");
    cy.get(".side-btn").contains("Reset").click();
    cy.get(".footswitch").should("contain", "Engage");
    cy.get(".knob-value").eq(0).should("contain", "0.00 st");
  });
});
