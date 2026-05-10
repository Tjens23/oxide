/**
 * @name Path traversal via package name
 * @kind path-problem
 * @severity error
 * @id rust/path-traversal
 */

import rust
import codeql.rust.dataflow.DataFlow
import codeql.rust.dataflow.TaintTracking

// Source: any function parameter — user-controlled strings enter through CLI arg parsing
class UserInputSource extends DataFlow::Node {
  UserInputSource() { this instanceof DataFlow::ParameterNode }
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
}

module PathTaint = TaintTracking::Global<PathTaintConfig>;

import PathTaint::PathGraph

from PathTaint::PathNode source, PathTaint::PathNode sink
where PathTaint::flowPath(source, sink)
select sink.getNode(), source, sink,
  "Path traversal: user input reaches file path construction."