# Spec: coupon2

订单优惠券模块（理想形态版：意图是唯一真相源，Rust 是生成物）。支持百分比券和固定金额券。资格判定返回状态码：0 可用，1 低于门槛，2 已过期，3 次数用尽，4 订单已用券。折扣逐项记账：百分比部分和固定部分分开，总额等于分项之和。金额一律整数分。

## Decisions

| id | question | options | chosen | source |
|---|---|---|---|---|
| stacking | 一张订单能用几张券？ | one_per_order, stack_all | one_per_order | DEFAULT (accepted risk) |
| refund | 退款时已享折扣怎么处理？ | full_refund_only, prorated | full_refund_only | DEFAULT (accepted risk) |
| expiry_boundary | 过期日当天还能用吗？ | end_of_day, start_of_day | end_of_day | DEFAULT (accepted risk) |
| per_user_limit | 每个用户能用同一张券几次？ | 1, 3, unlimited | 1 | DEFAULT (accepted risk) |
| below_threshold | 订单金额低于门槛时怎么办？ | error_keep_coupon, error_consume, silent_zero | error_keep_coupon | DEFAULT (accepted risk) |
| percent_cap | 百分比券有封顶吗？ | none, fixed_cents | none | DEFAULT (accepted risk) |

## Functions and their contracts

- **percent_discount**: requires [subtotal >= 0; pct >= 0 && pct <= 100], ensures [result >= 0; result <= subtotal]
- **discount_for**: requires [subtotal >= 0; value >= 0; kind == 0 || kind == 1], ensures [result.total == result.percent_part + result.fixed_part; result.total >= 0 && result.total <= subtotal]
- **final_price**: requires [subtotal >= 0; value >= 0; kind == 0 || kind == 1], ensures [result >= 0 && result <= subtotal]
- **check_eligible**: requires [subtotal >= 0; threshold >= 0; uses >= 0; limit >= 0], ensures [result >= 0 && result <= 4]

## Properties

- **total_bounds**: 折扣总额恒在 [0, 订单小计] 内，固定券面额大于小计时按小计封顶 (evidence: scenario)
- **line_items**: 折扣分项逐项正确：百分比券只有百分比部分，固定券只有固定部分，总额等于分项之和。只对总额的检查不算数 (evidence: scenario)
- **threshold_error**: 低于门槛返回状态 1（error_keep_coupon：资格函数不消耗次数，消耗发生在店面层） (evidence: scenario)
- **stacking**: 订单已用过券时返回状态 4（one_per_order） (evidence: scenario)
- **expiry_boundary**: 过期日当天可用，次日不可用（end_of_day） (evidence: scenario)
- **user_limit**: 次数达到上限返回状态 3，未达上限返回 0 (evidence: scenario)
- **percent_cap**: 无封顶时百分比折扣按比例算；给定封顶时不超过封顶 (evidence: scenario)
- **percent_discount_bounds**: percent_discount 对定义域内所有输入满足 0 <= result <= subtotal (evidence: verify)
- **refund_rule**: 退款时已享折扣仅在全额退款时返还（full_refund_only） (evidence: none) — accepted without evidence: 退款流程在店面层实现，不在本模块。风险由店面层测试承担，此处签字接受无证据。
