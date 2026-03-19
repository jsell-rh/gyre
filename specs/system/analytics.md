# Product Analytics

- **Built-in analytics** (PostHog-like) - not a third-party integration, part of Gyre.
- Track user behavior, feature usage, funnels, retention, etc.
- Analytics data must be **agent-consumable** - agents can query analytics to make decisions about features:
  - Is this feature being used? By whom? How often?
  - Did this change improve or degrade a metric?
  - Should this feature flag be promoted or rolled back?
- Closes the loop: agents ship features → analytics measures impact → agents decide next action.
