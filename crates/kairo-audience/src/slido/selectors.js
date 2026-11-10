var qaSelectors = (() => {
  var __defProp = Object.defineProperty;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
  var __export = (target, all) => {
    for (var name in all)
      __defProp(target, name, { get: all[name], enumerable: true });
  };
  var __copyProps = (to, from, except, desc) => {
    if (from && typeof from === "object" || typeof from === "function") {
      for (let key of __getOwnPropNames(from))
        if (!__hasOwnProp.call(to, key) && key !== except)
          __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
    }
    return to;
  };
  var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

  // ../kairo-presenter/src/main/services/audience/sources/slido-selectors.ts
  var slido_selectors_exports = {};
  __export(slido_selectors_exports, {
    SLIDO_SELECTORS: () => SLIDO_SELECTORS,
    extractSlidoQuestions: () => extractSlidoQuestions,
    pendingQueueButton: () => pendingQueueButton,
    readSlidoPage: () => readSlidoPage
  });
  var SLIDO_SELECTORS = {
    publicCards: ".question-item[data-qid]",
    publicText: '[data-testid="question-text"], .question-item__body',
    publicAuthor: '[data-testid="question-author"]',
    publicVotes: ".score__btn--plus > span",
    moderatorCards: '[data-question-id][data-testid="question"]',
    moderatorText: '[class*="questionText-"]',
    moderatorAuthor: '[data-testid="user-name"]',
    moderatorVotes: '[class*="srOnly-"]',
    moderatorCard: "[data-item-question-card]",
    pendingButton: "button",
    joinInput: 'input[placeholder="enter code here"]',
    joinButton: "#search__btn-submit",
    /** The public page's ask box: present means the Q&A page loaded, even with no questions yet. */
    publicAskBox: '[placeholder*="question" i]'
  };
  var NOT_FOUND = /no such slido/i;
  var TITLE_PREFIX = /^(?:Q&A|Polls|Quiz|Ideas|Survey)\s+on\s+/i;
  function readSlidoPage(document, location, mode) {
    const onEvent = location.pathname.startsWith("/event/");
    const questions = onEvent ? extractSlidoQuestions(document, mode) : [];
    return {
      questions,
      empty: onEvent && mode === "public" && !questions.length && !!document.querySelector(SLIDO_SELECTORS.publicAskBox),
      notFound: !onEvent && NOT_FOUND.test(document.body?.innerText ?? ""),
      eventName: onEvent && location.hostname === "app.sli.do" && TITLE_PREFIX.test(document.title) ? document.title.replace(TITLE_PREFIX, "").trim() || null : null
    };
  }
  function extractSlidoQuestions(document, mode) {
    const moderator = mode === "moderator";
    const cards = document.querySelectorAll(moderator ? SLIDO_SELECTORS.moderatorCards : SLIDO_SELECTORS.publicCards);
    const result = /* @__PURE__ */ new Map();
    for (const card of cards) {
      if (!card.getClientRects().length || card.closest('[aria-hidden="true"], [hidden]')) continue;
      const text = card.querySelector(moderator ? SLIDO_SELECTORS.moderatorText : SLIDO_SELECTORS.publicText)?.innerText?.trim();
      const externalId = card.getAttribute(moderator ? "data-question-id" : "data-qid");
      const author = card.querySelector(moderator ? SLIDO_SELECTORS.moderatorAuthor : SLIDO_SELECTORS.publicAuthor)?.innerText?.trim();
      let voteText = card.querySelector(moderator ? SLIDO_SELECTORS.moderatorVotes : SLIDO_SELECTORS.publicVotes)?.innerText?.trim();
      if (moderator && !voteText) voteText = card.closest(SLIDO_SELECTORS.moderatorCard)?.getAttribute("aria-label")?.match(/([\d,]+) votes?\s*$/)?.[1];
      const voteMatch = voteText?.match(/^([\d,\s]+)(?:\s+votes?)?$/);
      if (!text || !externalId || !voteMatch) continue;
      const votes = Number(voteMatch[1].replace(/[,\s]/g, ""));
      if (!Number.isSafeInteger(votes) || votes < 0) continue;
      result.set(externalId, { externalId, text, author: !author || author === "Anonymous" ? null : author, votes });
    }
    return [...result.values()];
  }
  function pendingQueueButton(document) {
    return [...document.querySelectorAll(SLIDO_SELECTORS.pendingButton)].find((button) => /^Pending\s+[1-9]\d*$/.test(button.innerText.trim()) && !button.disabled) ?? null;
  }
  return __toCommonJS(slido_selectors_exports);
})();
