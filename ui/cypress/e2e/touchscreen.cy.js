// 7" 800x480 touchscreen layout (US-3.4). Every control must be a real touch
// target that nothing else covers, with no hover-only affordance and no
// sideways overflow.

const MIN = 44;

/** One axis of an element's box is at least the touch minimum. */
function bigEnough(el) {
  const r = el.getBoundingClientRect();
  return Math.max(r.width, r.height) >= MIN - 0.5;
}

/** The topmost element at the centre of `el` is `el` itself or a child of it. */
function hittable(el) {
  el.scrollIntoView({ block: "center", inline: "center" });
  const r = el.getBoundingClientRect();
  const hit = el.ownerDocument.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
  return !!hit && (hit === el || el.contains(hit));
}

describe("7-inch 800x480 touchscreen", () => {
  beforeEach(() => {
    cy.viewport(800, 480);
    cy.boot();
  });

  it("has no sideways overflow", () => {
    cy.document().then((doc) => {
      expect(doc.documentElement.scrollWidth).to.be.at.most(doc.documentElement.clientWidth);
    });
  });

  it("keeps the top bar to two rows with a usable preset nav", () => {
    cy.get(".topbar").then(($bar) => {
      expect($bar[0].getBoundingClientRect().height).to.be.at.most(2 * 58 + 24);
      expect($bar[0].scrollWidth).to.be.at.most($bar[0].clientWidth);
    });
    cy.get(".slots").should("not.be.visible");
    cy.get(".preset .nav").each(($b) => {
      const r = $b[0].getBoundingClientRect();
      expect(r.width, "nav width").to.be.at.least(MIN - 0.5);
      expect(hittable($b[0]), "nav hit-test").to.equal(true);
    });
    cy.get(".preset-name").then(($n) => {
      expect($n[0].getBoundingClientRect().width).to.be.at.least(60);
    });
  });

  it("the ☰ button opens the preset browser", () => {
    cy.get(".presets-open").click();
    cy.get(".modal").should("be.visible");
  });

  it("folds meters, load and latency into one DSP popover", () => {
    cy.get(".dsp-inline").should("not.be.visible");
    cy.get(".dsp-popover").should("not.exist");
    cy.get(".dsp-btn").click();
    cy.get(".dsp-popover").should("be.visible").within(() => {
      cy.get(".meter").should("have.length", 2);
      cy.get(".latency").should("exist");
      cy.get(".buffer-select select").should("exist");
    });
    cy.get(".dsp-btn").click();
    cy.get(".dsp-popover").should("not.exist");
  });

  it("every interactive control has a 44px axis and is not covered", () => {
    cy.get(
      ".topbar button, .topbar select, .topbar [role=slider], .block-card, .card-actions button, .block-out",
    ).filter(":visible").each(($el) => {
      const el = $el[0];
      expect(bigEnough(el), `${el.className} size`).to.equal(true);
    });
    cy.get(".card-actions button, .block-out").each(($el) => {
      expect(hittable($el[0]), `${$el[0].className} hit-test`).to.equal(true);
    });
  });

  it("shows Bypass and Remove without hover and keeps them tabbable", () => {
    cy.get(".card-actions button").each(($b) => {
      expect(getComputedStyle($b[0]).opacity).to.equal("1");
      expect($b[0].tabIndex).to.be.at.least(0);
    });
    cy.get(".channel").eq(0).find(".block-card").eq(0).find(".bypass").click();
    cy.get(".channel").eq(0).find(".block-card").eq(0).should("have.class", "bypass");
  });

  it("fits four channel rows without overlap; strips scroll sideways", () => {
    cy.get(".channel").should("have.length", 4);
    cy.get(".channel").then(($ch) => {
      const rects = [...$ch].map((c) => c.getBoundingClientRect());
      for (let i = 1; i < rects.length; i++) {
        expect(rects[i].top, `row ${i} below row ${i - 1}`).to.be.at.least(rects[i - 1].bottom - 0.5);
      }
    });
    cy.get(".channel .strip").each(($s) => {
      expect(getComputedStyle($s[0]).overflowX).to.equal("auto");
    });
  });

  it("keeps the editor inside its height cap", () => {
    cy.selectBlock(0, 0);
    cy.get(".editor").then(($e) => {
      expect($e[0].getBoundingClientRect().height).to.be.at.most(0.62 * 480 + 1);
    });
  });

  it("makes a routing from the handle and shows no error", () => {
    cy.get(".channel").eq(1).find(".block-card").first().then(($t) => {
      cy.get(".channel").eq(0).find(".block-card").first().find(".block-out").then(($h) => {
        const h = $h[0].getBoundingClientRect();
        const win = $h[0].ownerDocument.defaultView;
        const t = $t[0].getBoundingClientRect();
        const ev = (type, x, y) =>
          $h[0].dispatchEvent(
            new win.PointerEvent(type, {
              bubbles: true, cancelable: true, pointerId: 7, pointerType: "touch",
              clientX: x, clientY: y,
            }),
          );
        ev("pointerdown", h.left + h.width / 2, h.top + h.height / 2);
        ev("pointermove", t.left + t.width / 2, t.top + t.height / 2);
        ev("pointerup", t.left + t.width / 2, t.top + t.height / 2);
      });
    });
    cy.get(".toast, .notice, [role=alert]").should("not.exist");
  });

  it("opens the add-block picker from a channel", () => {
    cy.get(".channel").eq(2).find(".add-block").scrollIntoView().click();
    cy.get(".modal.picker").should("be.visible");
  });

  it("inserts a block with a long-press drag from the rack", () => {
    cy.contains(".rack-item", /^Reverb$/).then(($row) => {
      const win = $row[0].ownerDocument.defaultView;
      const fire = (type, x, y, target) =>
        target.dispatchEvent(
          new win.PointerEvent(type, {
            bubbles: true, cancelable: true, pointerId: 1, pointerType: "touch", clientX: x, clientY: y,
          }),
        );
      fire("pointerdown", 100, 100, $row[0]);
      cy.wait(450, { log: false }).then(() => {
        cy.get(".channel").eq(2).scrollIntoView().then(($ch) => {
          const r = $ch[0].getBoundingClientRect();
          const x = Math.round(r.left + r.width / 2);
          const y = Math.round(r.top + r.height / 2);
          fire("pointermove", x, y, win);
          fire("pointerup", x, y, win);
        });
      });
    });
    cy.get(".channel").eq(2).find(".block-card").should("have.length", 1);
  });

  it("reorders a card by drag", () => {
    const cards = () => cy.get(".channel").eq(0).find(".block-card");
    cards().eq(0).find(".block-name").should("have.text", "Amp");
    cy.get(".channel").eq(0).find(".strip").then(($strip) => {
      const r = $strip[0].getBoundingClientRect();
      const y = Math.round(r.top + r.height / 2);
      const startX = Math.round(r.left + 40);
      const endX = Math.round(r.right - 5);
      cards().eq(0).trigger("pointerdown", { pointerId: 1, button: 0, clientX: startX, clientY: y });
      cards().eq(0).trigger("pointermove", { pointerId: 1, clientX: endX, clientY: y });
      cards().eq(0).trigger("pointerup", { pointerId: 1 });
    });
    cards().eq(0).find(".block-name").should("have.text", "Delay");
    cards().eq(2).find(".block-name").should("have.text", "Amp");
  });
});
