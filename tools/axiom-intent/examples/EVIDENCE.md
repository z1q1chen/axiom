# Evidence: coupon2

Target: generated Rust (`coupon2.rs`, artifact, do not edit). Contracts in ledger: 19. Decisions: 0 answered, 6 on default. Integrity: IN-LOOP.

| property | tier | result | note |
|---|---|---|---|
| total_bounds | runtime-checked | PASS | scenario snippet against generated Rust |
| line_items | runtime-checked | PASS | scenario snippet against generated Rust |
| threshold_error | runtime-checked | PASS | scenario snippet against generated Rust |
| stacking | runtime-checked | PASS | scenario snippet against generated Rust |
| expiry_boundary | runtime-checked | PASS | scenario snippet against generated Rust |
| user_limit | runtime-checked | PASS | scenario snippet against generated Rust |
| percent_cap | runtime-checked | PASS | scenario snippet against generated Rust |
| percent_discount_bounds | bounded | PASS | exhausted: 420 cases, 580 excluded by requires |
| refund_rule | none | UNCHECKED (accepted) | 退款流程在店面层实现，不在本模块。风险由店面层测试承担，此处签字接受无证据。 |

## Unchecked / accepted risks

- property `refund_rule` accepted without evidence
- decision `stacking` still on default `one_per_order`
- decision `refund` still on default `full_refund_only`
- decision `expiry_boundary` still on default `end_of_day`
- decision `per_user_limit` still on default `1`
- decision `below_threshold` still on default `error_keep_coupon`
- decision `percent_cap` still on default `none`
