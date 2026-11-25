// Shared Cypress commands for the E2E suite.
//
// `cy.boot()` is the entry point of nearly every test: it loads the app against
// the fake engine and waits until the rig has rendered. The fake engine resets
// to its seeded state on every connection, so each test starts from the same
// rig.

Cypress.Commands.add("boot", () => {
  cy.visit("/", {
    onBeforeLoad(win) {
      // The preset browser confirms deletions. Accept them deterministically
      // instead of depending on Cypress's dialog handling.
      win.confirm = () => true;
    },
  });
  cy.get(".topbar", { timeout: 15000 }).should("exist");
});

Cypress.Commands.add("openPresetBrowser", () => {
  cy.get(".presets-open").click();
  cy.get(".modal").should("be.visible");
});

/** Click a block card in a channel row and wait for the editor to show it. */
Cypress.Commands.add("selectBlock", (channelIndex, blockIndex) => {
  cy.get(".channel")
    .eq(channelIndex)
    .find(".block-card")
    .eq(blockIndex)
    .click();
  cy.get(".editor-name").should("exist");
});

/**
 * Dispatch a pointer event with an explicit `pointerType`.
 *
 * Cypress's `.trigger()` builds the event without one, and the app's gesture
 * code treats an absent `pointerType` as a mouse — so a touch test has to say
 * `pointerType: "touch"` or the long press never starts.
 */
Cypress.Commands.add("pointer", { prevSubject: "element" }, (el, type, x, y) => {
  const doc = el?.ownerDocument ?? (el?.[0]?.ownerDocument ?? window.document);
  const win = doc.defaultView ?? window;
  const e = new win.PointerEvent(type, {
    bubbles: true,
    cancelable: true,
    pointerId: 1,
    pointerType: "mouse",
    clientX: x,
    clientY: y,
  });
  (el?.[0] ?? el).dispatchEvent(e);
});

Cypress.Commands.add("key", { prevSubject: "element" }, (el, type, key) => {
  // `el` may be a plain DOM node or a jQuery/Cypress wrapper; unwrap it.
  const doc = el?.ownerDocument ?? (el[0]?.ownerDocument ?? window.document);
  const win = doc.defaultView ?? window;
  const e = new win.KeyboardEvent(type, {
    bubbles: true,
    cancelable: true,
    key,
    code: key,
    keyCode: key === "Enter" ? 13 : 0,
    which: key === "Enter" ? 13 : 0,
  });
  (el?.[0] ?? el).dispatchEvent(e);
});
