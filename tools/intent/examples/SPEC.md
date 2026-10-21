# Spec: coupon

订单优惠券模块。支持百分比券和固定金额券。每张券有使用门槛、过期日和每用户次数限制。折扣必须逐项可核对：百分比部分和固定部分分开记账，总额等于分项之和。金额一律用整数分。

## Decisions

| id | question | options | chosen | source |
|---|---|---|---|---|
| stacking | 一张订单能用几张券？ | one_per_order, stack_all | one_per_order | DEFAULT (accepted risk) |
| refund | 退款时已享折扣怎么处理？ | full_refund_only, prorated | full_refund_only | DEFAULT (accepted risk) |
| expiry_boundary | 过期日当天还能用吗？ | end_of_day, start_of_day | end_of_day | DEFAULT (accepted risk) |
| per_user_limit | 每个用户能用同一张券几次？ | 1, 3, unlimited | 1 | DEFAULT (accepted risk) |
| below_threshold | 订单金额低于门槛时怎么办？ | error_keep_coupon, error_consume, silent_zero | error_keep_coupon | DEFAULT (accepted risk) |
| percent_cap | 百分比券有封顶吗？ | none, fixed_cents | none | DEFAULT (accepted risk) |

## Properties

- **total_bounds**: 折扣总额恒在 [0, 订单小计] 内，固定券面额大于小计时按小计封顶 (evidence: scenario)
- **line_items**: 折扣分项逐项正确：百分比券只有百分比部分，固定券只有固定部分，总额等于分项之和。只对总额的检查不算数 (evidence: scenario)
- **threshold_error**: 低于门槛返回错误状态，且不消耗券（error_keep_coupon） (evidence: scenario)
- **stacking**: 订单已用过券时第二张被拒（one_per_order） (evidence: scenario)
- **expiry_boundary**: 过期日当天可用，次日不可用（end_of_day） (evidence: scenario)
- **user_limit**: 同一用户第二次使用被拒，其他用户不受影响（上限 1 次） (evidence: scenario)
- **percent_cap**: 无封顶时百分比折扣按比例算；给定封顶时不超过封顶 (evidence: scenario)
- **percent_discount_bounds**: percent_discount 对定义域内所有输入满足 0 <= result <= subtotal (evidence: verify)
