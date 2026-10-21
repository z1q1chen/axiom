# Evidence: coupon

Contracts in ledger: 11. Decisions: 0 answered, 6 on default.

| property | tier | result | note |
|---|---|---|---|
| total_bounds | runtime-checked | PASS | scenario checks in the program's main |
| line_items | runtime-checked | PASS | scenario checks in the program's main |
| threshold_error | runtime-checked | PASS | scenario checks in the program's main |
| stacking | runtime-checked | PASS | scenario checks in the program's main |
| expiry_boundary | runtime-checked | PASS | scenario checks in the program's main |
| user_limit | runtime-checked | PASS | scenario checks in the program's main |
| percent_cap | runtime-checked | PASS | scenario checks in the program's main |
| percent_discount_bounds | bounded | PASS | exhausted on the declared domain (168 cases) |

## Unchecked / accepted risks

- decision `stacking` still on default `one_per_order`
- decision `refund` still on default `full_refund_only`
- decision `expiry_boundary` still on default `end_of_day`
- decision `per_user_limit` still on default `1`
- decision `below_threshold` still on default `error_keep_coupon`
- decision `percent_cap` still on default `none`
