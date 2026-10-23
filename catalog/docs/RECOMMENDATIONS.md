<div align="center">

# Recommendations

**Personal picks, useful categories, and saved results that work through upstream outages.**

---

</div>

## Overview

The catalog finds possible movies and shows, ranks them, and saves the results. The API uses those saved results to build each profile's Home page.

Recommendations use fixed rules and math. They don't require an AI service.

## What Shapes Your Picks

| Input | How it helps |
|---|---|
| Favorites | Each favorite starts with weight 5. |
| Selected genres | Each genre list contributes weight 2. |
| Meaningful viewing | Movies and shows you spend time watching become starting points. |
| Recent activity | Older viewing has less influence. |

The API leaves hidden titles and titles marked **Don't use for my taste** out of the inputs. The original favorites and watch history remain saved.

### Meaningful Viewing

- **Movies:** at least 10 watched minutes, plus completion or at least 20% of runtime watched.
- **Series:** at least 20 watched minutes, plus two episodes watched for 10 minutes each or at least 45 minutes watched in total.

Viewing influence halves every 14 days. Viewing older than 60 days stops contributing. Favorites do not use that decay.

## Ranking

The existing score remains:

```text
Score = Source contributions + Agreement bonus + Rating bonus
```

- A high position in a related-title list contributes more than a low position.
- Appearing in several lists adds an agreement bonus.
- A good audience rating adds a small bonus.

The featured picks also try to space out related films. This uses title-name matching, so it is an approximation.

## Categories

The background browse refresh adds these topic rows:

| Row | Membership rule |
|---|---|
| Heist movies | TMDB's exact heist keyword |
| Time travel | TMDB's exact time-travel keyword |
| Detective stories | TMDB's exact detective keyword |
| Romantic comedies | Comedy and Romance genres |
| Historical dramas | Drama and History genres |
| Animated adventures | Animation and Adventure genres |
| Documentaries | Documentary genre |
| Mystery series | Mystery genre |
| Fantasy films | Fantasy genre |
| Under 100 minutes | Movie runtime from 1 to 99 minutes |
| Before 2000 | Movie release date before 2000 |

These filters use [TMDB Discover](https://developer.themoviedb.org/reference/discover-movie) and [keyword search](https://developer.themoviedb.org/reference/search-keyword). Keyword IDs are looked up, not guessed. If a keyword lookup fails, the worker skips that topic and keeps its saved version.

Recommendation genre searches now allow older and international titles. Anime keeps its Japanese-original-language rule. The existing onboarding choices keep their saved IDs.

## Personal Rows

- **Picked for you:** titles from the ranked personal pool.
- **Because you liked…:** suggestions connected to a favorite.
- **Because you watched…:** suggestions connected to meaningful viewing.

The contextual-row helper is now used by the recommendation builder.

The `SerendipityMix` helper remains available, but the production page does **not** promise a 70/30 mix. The API instead builds a separate **Try something different** row from titles outside the ranked taste pool.

## Building the Home Page

The API reads saved catalog data and current profile choices. It:

1. Applies format filters and hidden-title choices.
2. Leaves completed movies out of new suggestions.
3. Keeps a whole series visible after a single completed episode.
4. Chooses useful topic and personal rows.
5. Removes repeated titles across the page.
6. Reserves one featured title so it does not repeat in a row.

No Home-page request waits for TMDB or checks playback sources. Being listed does not guarantee playback availability.

## Refresh Timing

| Situation | Timing |
|---|---|
| Active profile | Every 24 hours |
| Less-active profile | Every seven days |
| Changed taste inputs | Queued for an update |
| General browse and topic rows | Six-hour shelf refresh |
| Due-profile sweep | Every 15 minutes |

A failed refresh keeps the previous results. Hidden-title and hidden-category choices apply when the API reads the page, even if the saved recommendations are old.

New topic rows appear after a successful browse refresh and API snapshot sync. Static checks do not prove live recommendation quality or playback.
