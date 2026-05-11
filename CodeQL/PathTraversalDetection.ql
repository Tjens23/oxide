/**
 * @name Path traversal via package name
 * @kind path-problem
 * @severity error
 * @id rust/path-traversal
 */

import rust
import codeql.rust.dataflow.DataFlow
import codeql.rust.dataflow.TaintTracking

// Source: HTTP response bytes from the registry — the only external attacker-controlled
// binary data that flows into file-system path construction in this codebase.
class UserInputSource extends DataFlow::Node {
  UserInputSource() {
    exists(MethodCallExpr mc |
      mc.getIdentifier().getText() = "bytes" and
      this.asExpr() = mc
    )
  }
}

// Sink: first argument of file-system functions that open or create paths
class PathSink extends DataFlow::Node {
  PathSink() {
    exists(Call c |
      c.getStaticTarget()
        .(Function)
        .getCanonicalPath()
        .regexpMatch(".*(fs::write|fs::read_to_string|fs::create_dir_all|fs::create_dir|File::create|File::open).*") and
      this.asExpr() = c.getPositionalArgument(0)
    )
  }
}

module PathTaintConfig implements DataFlow::ConfigSig {
  predicate isSource(DataFlow::Node n) { n instanceof UserInputSource }
  predicate isSink(DataFlow::Node n) { n instanceof PathSink }

  predicate isBarrier(DataFlow::Node n) {
    // Paths rebuilt via .filter(...).collect() strip unsafe components and are sanitized.
    // This recognises the zip-slip fix in extract_tarball_strip.
    exists(MethodCallExpr collect, MethodCallExpr filter |
      collect.getIdentifier().getText() = "collect" and
      filter.getIdentifier().getText() = "filter" and
      collect.getReceiver() = filter and
      n.asExpr() = collect
    )
  }
}

module PathTaint = TaintTracking::Global<PathTaintConfig>;

import PathTaint::PathGraph

from PathTaint::PathNode source, PathTaint::PathNode sink
where PathTaint::flowPath(source, sink)
select sink.getNode(), source, sink,
  "Path traversal: user input reaches file path construction."