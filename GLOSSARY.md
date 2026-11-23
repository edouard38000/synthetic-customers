# Glossary

The words this repo uses, in code, tests, issues and reports.

- **Concept**: the product idea being tested: a text, optionally with an image. One run can test several Concepts (two versions, two prices, two names) to compare them.
- **Synthetic customer**: one AI-played persona with a value for each attribute of the Customer mix (for example 35–54, woman, middle income, Midwest). Each one is asked separately, in its own AI call, whether they would buy the Concept.
- **Customer mix**: the confirmed shares of ages, genders, incomes and regions the Synthetic customers are sampled from. The user confirms it before anything runs.
- **Purchase intent**: the answer on the 1–5 scale (1 = would definitely not buy, 5 = would definitely buy). Each answer is scored as a distribution over the five ratings; its expected value is the customer's Purchase intent.
- **Anchor set**: five reference statements, one per Purchase intent rating. SSR compares an answer to each statement of each Anchor set.
- **Scorer**: the embedding model that turns answers and anchor statements into vectors for SSR: local (default, no key), OpenAI or Voyage. `fake` exists for tests only.

Method: semantic similarity rating (SSR), arXiv 2510.08338 eq. 8.
