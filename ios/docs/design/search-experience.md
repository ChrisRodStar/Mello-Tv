# Search Experience

## Design Direction & Cohesion

Search has been redesigned to match the Netflix iOS search experience, featuring horizontal categorized shelves, pure 2:3 poster artwork with no under-card text labels, and smooth dual-axis navigation.

### 1. Pure Artwork Treatment (No Under-Card Text)
- As in Netflix, title name, year, and media type labels below posters have been eliminated:
  - The poster artwork already communicates identity and aesthetic directly.
  - Runtime, year, media type, and synopsis belong on the Title Details sheet upon selection.
- Cards maintain the compact 2:3 vertical aspect ratio (108 pt width × 162 pt height on iPhone, expanding to 148 pt under Dynamic Type accessibility sizes).
- 6-point corner radius with a 0.5-point subtle border (`white 8%`) and soft drop shadow.
- Interactive press feedback with `CardScaleButtonStyle(scale: 0.96)`.
- VoiceOver accessibility labels retain full title, media type, and release year context for screen readers.

### 2. Netflix-Style Category Shelves
- Rather than a flat vertical grid, search results are organized into vertically-stacked horizontal shelves:
  1. **Top Results:** Direct search matches from `api.search(query)` scrolling horizontally with automatic continuous pagination.
  2. **Media Type Groupings:** When search results contain both films and series (e.g. "Spider-Man Movies" and "Spider-Man TV Shows"), distinct dedicated shelves break down the formats.
  3. **Categorized Discovery Shelves:** Relevant catalog shelves from `api.home()` are ranked and matched against the query terms and the genres of the search results (e.g. "Action & adventure", "Sci-fi & fantasy", "Popular movies", "Trending this week").
- Each shelf features a bold category heading (`.font(.headline.weight(.bold))`) with 8-point card spacing and 16-point edge insets.
- At standard iPhone width (~393 pt), ~3.2 cards are visible at a glance, naturally revealing the fourth card edge to invite horizontal swiping.

### 3. Restrained Header & Interactive Dismissal
- Edge-to-edge search bar in a 42-point charcoal card container (`#1c1c1e`) with 10-point rounded corners.
- Red Netflix-inspired accent cursor and selection tint (`#e50914`).
- Animated "Cancel" button and clear button (`xmark.circle.fill`) with 44-point touch targets.
- Interactive keyboard dismissal (`.scrollDismissesKeyboard(.interactively)`) collapses the keyboard on drag to maximize artwork visibility.

### 4. Centered Empty & Feedback States
- Idle, loading, "no titles found", and retry error views are vertically and horizontally centered in the viewport between the header and bottom tab bar, dynamically adapting as the software keyboard appears and dismisses.

### 5. Multi-Layer Relevance Ranking (SearchRanker)
- Direct API results are scored and re-ranked using an information-retrieval model based on Netflix and BM25 principles:
  1. **Exact Normalized Match (+10,000 pts):** Titles matching the normalized query string exactly receive the highest priority.
  2. **Leading Article Normalization (+9,500 pts):** Strips "The", "A", and "An" so titles like "The Supernatural" match cleanly.
  3. **Prefix Matching (+4,000 pts):** Titles that start with the query tokens take precedence over titles where the word appears in the middle.
  4. **Multi-Token Coverage (+1,000 pts):** Multi-word queries reward titles containing all tokens regardless of order.
  5. **Title Coverage Ratio (+2,000 pts × ratio):** BM25 length normalization penalizes long titles where the query was only an incidental subtitle (e.g. Crayon Shin-chan).
  6. **Quality & Popularity Priors (+50 pts × rating):** Established, highly rated titles break ties over obscure unrated entries.

### 6. Concurrency & Performance
- Debounced query dispatch (300 ms) protected by `UUID` request tokens.
- Outdated or cancelled search responses cannot overwrite newer queries.
- Catalog sections are cached in `SearchStore` to ensure instant categorization on subsequent queries.
