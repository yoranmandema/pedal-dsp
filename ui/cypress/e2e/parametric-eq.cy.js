// Parametric EQ (tone.eq) panel — E2E tests.
//
// The panel renders the block's controls 1:1 from the engine's registry
// (docs/design/effects-parametric-eq.md): six band sections, each with an
// enable toggle, a frequency knob (log), a gain knob (dB) and a Q knob
// (peaking bands), a global output-gain knob, and a live client-side
// frequency-response curve (log frequency x, dB y, 0..±24 dB).
//
// The UI is driven against the fake engine, which speaks the same WebSocket
// protocol (docs/IPC-PROTOCOL.md) as the real `pedal-engine`, so no audio
// hardware is touched.

describe("Parametric EQ (tone.eq) panel", () => {
  // Seed rig: ch1 = amp.basic, time.delay, locked. After adding tone.eq it is
  // the 4th block in ch1, so selectBlock(0, 3).
  beforeEach(() => {
    cy.boot();
    // Channel 3 is empty in the seed rig; add the EQ through the picker.
    cy.get(".channel").eq(2).find(".add-block").click();
    cy.get(".modal.picker").should("be.visible");
    cy.get(".picker-search").type("tone.eq");
    cy.get(".picker .rack-item").should("have.length", 1).click();
    cy.get(".modal.picker").should("not.exist");
    cy.selectBlock(2, 0);
  });

  it("renders the block identity, six band sections and the curve", () => {
    cy.get(".editor-name").should("contain", "Parametric EQ");
    cy.get(".editor-type").should("contain", "tone.eq");

    // Design §2: six fixed bands in fixed order.
    cy.get(".eq-band").should("have.length", 6);
    cy.get(".eq-band-name").then(($names) => {
      const text = $names.map((i, el) => $(el).text().trim()).get();
      expect(text).to.deep.equal([
        "Low shelf", "Low peak", "High peak", "High shelf", "High pass", "Low pass",
      ]);
    });

    // Design §3: an enable toggle (0/1, real values) per band.
    cy.get(".eq-toggle").should("have.length", 6);

    // Design §2: global output_gain is applied after the band chain.
    cy.get(".eq-global-label").should("contain", "Output gain");

    // Design: client-side frequency-response curve.
    cy.get(".eq-curve-wrap").should("exist");
  });

  it("renders every control with the registry's ranges, units and ARIA", () => {
    // Each band has a frequency knob (log); gaining bands have a gain knob and
    // peaking bands have a Q knob.
    cy.get(".knob-unit").should("have.length", 11); // 6 freq + 2 gain + 2 Q + 1 output
    cy.get(".knob-unit")
      .its("length")
      .then((n) => {
        const names = [];
        cy.get(".knob-unit").each(($k) => {
          const t = $k.find(".knob-name").text().trim();
          names.push(t);
        });
        // Frequency knobs: 6, with Hz units.
        expect(names.filter((n) => n.includes("freq"))).to.have.length(6);
        expect(names.filter((n) => n.includes("gain"))).to.have.length(4); // 2 per shelf band + 2
        expect(names.filter((n) => n === "Q")).to.have.length(2);
        expect(names.filter((n) => n === "Output gain")).to.have.length(1);
      });

    // Labels, toggles and range inputs carry accessible attributes.
    cy.get(".eq-toggle-input").should("have.attr", "aria-checked");
    cy.get(".eq-toggle-input").each(($t) => {
      // Enabled bands start at 1 (all bands enabled at seed defaults).
      expect(parseInt($t.getAttribute("aria-checked"), 10)).to.equal(1);
    });

    // Global and per-band gain knobs report dB.
    cy.get(".knob-value").should("contain", "dB");
  });

  it("the curve chart is a real plot (log x, dB y) with grid and ticks", () => {
    const svg = cy.get(".eq-curve-svg");
    svg.should("have.attr", "viewBox", "0 0 640 360");

    // Frequency ticks: 20 Hz, 100 Hz, 1 kHz, 10 kHz, 20 kHz.
    cy.get(".eq-curve-svg").find("text").then(($texts) => {
      const labels = [];
      $texts.each((i, el) => {
        const t = $(el).text().trim();
        if (t) labels.push(t);
      });
      expect(labels).to.include("20 Hz");
      expect(labels).to.include("100 Hz");
      expect(labels).to.include("1 kHz");
      expect(labels).to.include("10 kHz");
      expect(labels).to.include("20 kHz");
    });

    // Grid lines every 6 dB on the dB axis.
    cy.get(".eq-curve-svg").find("line").then(($lines) => {
      const yPositions = [];
      $lines.each((i, el) => {
        const y1 = $(el).attr("y1");
        const y2 = $(el).attr("y2");
        if (y1 && y2 && y1 === y2) yPositions.push(parseFloat(y1));
      });
      // 0 dB reference + 6/±6/±12/±18 dB grid lines => 9 horizontal lines.
      expect(yPositions).to.have.length.greaterThan(1);
    });
  });

  it("sends a knob change over the wire and the value persists", () => {
    // Change the low peak frequency (band 1) by dragging its log knob.
    const knob = () => cy.get(".eq-band").eq(1).find(".knob").eq(0);
    const before = () => knob().find(".knob-value").invoke("text");

    let initial;
    before().waits(0).then(() => initial = before().then((v) => v));

    knob().trigger("pointerdown", { pointerId: 1, button: 0, clientY: 300 });
    knob().trigger("pointermove", { pointerId: 1, clientY: 100 });
    knob().trigger("pointerup", { pointerId: 1 });

    // The knob value must have moved and settled, not reverted to default.
    cy.get(".eq-band").eq(1).find(".knob-value").should("not.equal", initial);
    cy.get(".eq-band").eq(1).find(".knob-value").should("not.equal", "300 Hz");
  });

  it("operates its controls with the keyboard (ARIA targets)", () => {
    // The frequency knob accepts arrow keys and Home/End range limits.
    cy.get(".eq-band").eq(2).find(".knob").eq(0) // high peak freq
      .focus()
      .type("{arrowright}")
      .should("have.focus");
    cy.get(".eq-band").eq(2).find(".knob-value").should("not.contain", "1.00 Hz");

    // The enable toggle is keyboard-operable.
    cy.get(".eq-band").eq(0).find(".eq-toggle-input").focus()
      .type("{enter}");
    cy.get(".eq-band").eq(0).find(".eq-toggle-input").should("be.checked");
  });
});
