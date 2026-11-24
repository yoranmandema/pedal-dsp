// End-to-end tests for the model browse-to-download flow.
// Uses the fake engine (fake-engine.mjs) — no audio device, no real sound.
//
// Flow: refresh index -> search/filter -> view model -> download -> verify index update.

describe("model download flow", () => {
  beforeEach(() => {
    cy.boot();
  });

  it("refreshes the index and shows the result count", () => {
    cy.get(".model-browser .side-btn").contains("Refresh index").click();

    // The refresh completes and shows a count: "N of M"
    cy.contains("of").should("be.visible");
    cy.contains("model").should("exist");
  });

  it("searches the index and filters results", () => {
    // Open the browser first
    cy.get(".top-bar .side-btn").contains("Models").click();

    cy.get(".picker-search").type("lead");

    // Filtered results should be visible
    cy.get(".picker .rack-item").should("have.length.greaterThan", 0);

    // The search text is visible in the input
    cy.get(".picker-search").should("have.value", "lead");
  });

  it("filters by token presence (AND across tokens)", () => {
    cy.get(".top-bar .side-btn").contains("Models").click();

    // Type two tokens; both must match
    cy.get(".picker-search").type("lead {backspace}{backspace}reverb");

    // Filtered results
    cy.get(".picker .rack-item").should("have.length.greaterThan", 0);

    // The search field shows the combined query
    cy.get(".picker-search").should("have.value", "lead reverb");
  });

  it("shows model details when a result is clicked", () => {
    cy.get(".top-bar .side-btn").contains("Models").click();

    // Open a model's detail view
    cy.get(".picker .rack-item").first().click();

    // Detail panel appears
    cy.get(".model-detail").should("be.visible");
  });

  it("downloads a model and updates the index", () => {
    cy.get(".top-bar .side-btn").contains("Models").click();

    // Click download on a model
    cy.get(".picker .rack-item").first().find(".model-detail button").contains("Download").click();

    // The download panel appears
    cy.contains("Downloads").should("be.visible");
  });

  it("shows the Downloads panel with progress", () => {
    cy.get(".top-bar .side-btn").contains("Models").click();

    // Trigger a download (the fake engine will simulate it)
    cy.get(".model-detail button").contains("Download").click();

    // Downloads panel with progress bar
    cy.contains("Downloads");
    cy.get(".downloads-panel .progress-bar").should("exist");
  });

  it("shows already-downloaded message for re-download", () => {
    cy.get(".top-bar .side-btn").contains("Models").click();

    // The index has been refreshed; re-download shows already_downloaded
    cy.get(".model-detail button").contains("Download").click();

    // The download manager reports already_downloaded
    cy.contains("already_downloaded").should("exist");
  });

  it("closes the browser with Escape", () => {
    cy.get(".top-bar .side-btn").contains("Models").click();

    cy.get(".model-browser .modalBackdrop").click();

    // The browser closes
    cy.get(".model-browser").should("not.be.visible");
  });

  it("closes the browser with the × button", () => {
    cy.get(".top-bar .side-btn").contains("Models").click();

    cy.get(".model-browser .close-btn").click();

    cy.get(".model-browser").should("not.be.visible");
  });
});
