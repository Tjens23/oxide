/**
 * @name Unwrap on async operation result
 * @kind problem
 * @severity warning
 * @id rust/unwrap-on-async-result
 */

import rust

from MethodCallExpr unwrapCall
where
  unwrapCall.getIdentifier().getText() = "unwrap" and
  unwrapCall.getReceiver() instanceof AwaitExpr
select unwrapCall,
  "Calling .unwrap() on an awaited result can panic if the async operation fails; use '?' or match instead."
