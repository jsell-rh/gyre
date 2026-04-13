// go-callgraph computes a complete call graph for a Go project using CHA
// (Class Hierarchy Analysis) and outputs it as JSON to stdout.
//
// Usage: go-callgraph <repo-path>
//
// Output format: [{"from": "pkg.FuncName", "to": "pkg.OtherFunc"}, ...]
// Qualified names use the form: <import-path>.<FuncName> or <import-path>.<Type>.<Method>
package main

import (
	"encoding/json"
	"fmt"
	"go/types"
	"os"
	"strings"

	"golang.org/x/tools/go/callgraph/cha"
	"golang.org/x/tools/go/packages"
	"golang.org/x/tools/go/ssa"
	"golang.org/x/tools/go/ssa/ssautil"
)

// CallEdge represents a single caller->callee relationship.
type CallEdge struct {
	From string `json:"from"`
	To   string `json:"to"`
}

func main() {
	if len(os.Args) < 2 {
		fmt.Fprintf(os.Stderr, "usage: go-callgraph <repo-path>\n")
		os.Exit(1)
	}
	repoPath := os.Args[1]

	cfg := &packages.Config{
		Mode: packages.LoadAllSyntax,
		Dir:  repoPath,
	}

	pkgs, err := packages.Load(cfg, "./...")
	if err != nil {
		fmt.Fprintf(os.Stderr, "error loading packages: %v\n", err)
		os.Exit(1)
	}

	// Check for package errors (non-fatal — print to stderr but continue).
	for _, pkg := range pkgs {
		for _, e := range pkg.Errors {
			fmt.Fprintf(os.Stderr, "package error: %v\n", e)
		}
	}

	// Build SSA program from loaded packages.
	prog, ssaPkgs := ssautil.AllPackages(pkgs, ssa.InstantiateGenerics)
	prog.Build()

	// Filter out nil packages (can happen if a package had errors).
	var validPkgs []*ssa.Package
	for _, p := range ssaPkgs {
		if p != nil {
			validPkgs = append(validPkgs, p)
		}
	}
	_ = validPkgs

	// Run CHA call graph analysis.
	cg := cha.CallGraph(prog)

	// Collect the module path to filter edges to project-local functions only.
	var modulePath string
	for _, pkg := range pkgs {
		if pkg.Module != nil && pkg.Module.Path != "" {
			modulePath = pkg.Module.Path
			break
		}
	}
	if modulePath == "" {
		// Fallback: try go.mod in the repo path
		if goModData, err := os.ReadFile(repoPath + "/go.mod"); err == nil {
			for _, line := range strings.Split(string(goModData), "\n") {
				line = strings.TrimSpace(line)
				if strings.HasPrefix(line, "module ") {
					modulePath = strings.TrimSpace(strings.TrimPrefix(line, "module"))
					break
				}
			}
		}
	}
	if modulePath == "" {
		fmt.Fprintf(os.Stderr, "warning: could not determine module path; all edges will be included\n")
	}

	// Deduplicate edges.
	seen := make(map[[2]string]bool)
	var edges []CallEdge

	for fn, node := range cg.Nodes {
		if fn == nil || fn.Package() == nil {
			continue
		}
		fromPkg := fn.Package().Pkg
		if fromPkg == nil {
			continue
		}
		// Only include edges from project-local functions.
		if modulePath != "" && !strings.HasPrefix(fromPkg.Path(), modulePath) {
			continue
		}
		fromName := qualifiedName(fn)
		if fromName == "" {
			continue
		}
		for _, edge := range node.Out {
			callee := edge.Callee.Func
			if callee == nil || callee.Package() == nil {
				continue
			}
			calleePkg := callee.Package().Pkg
			if calleePkg == nil {
				continue
			}
			// Only include edges to project-local functions.
			if modulePath != "" && !strings.HasPrefix(calleePkg.Path(), modulePath) {
				continue
			}
			toName := qualifiedName(callee)
			if toName == "" {
				continue
			}
			key := [2]string{fromName, toName}
			if seen[key] {
				continue
			}
			seen[key] = true
			edges = append(edges, CallEdge{From: fromName, To: toName})
		}
	}

	if edges == nil {
		edges = []CallEdge{}
	}

	enc := json.NewEncoder(os.Stdout)
	if err := enc.Encode(edges); err != nil {
		fmt.Fprintf(os.Stderr, "error encoding JSON: %v\n", err)
		os.Exit(1)
	}
}

// qualifiedName returns the qualified name of an SSA function in the format
// that matches the Go extractor's naming scheme:
//   - Function: <import-path>.<FuncName>
//   - Method:   <import-path>.<TypeName>.<MethodName>
//
// Returns empty string for functions that cannot be meaningfully named
// (e.g., anonymous functions, init, wrappers).
func qualifiedName(fn *ssa.Function) string {
	if fn.Parent() != nil {
		// Anonymous function / closure — skip.
		return ""
	}

	pkg := fn.Package()
	if pkg == nil || pkg.Pkg == nil {
		return ""
	}
	pkgPath := pkg.Pkg.Path()

	// For methods, fn.Signature.Recv() is set.
	recv := fn.Signature.Recv()
	if recv != nil {
		typeName := receiverTypeName(recv.Type())
		if typeName == "" {
			return ""
		}
		return pkgPath + "." + typeName + "." + fn.Name()
	}

	// Plain function.
	return pkgPath + "." + fn.Name()
}

// receiverTypeName extracts the base type name from a receiver type,
// stripping pointer indirection.
func receiverTypeName(t types.Type) string {
	// Strip pointer.
	if ptr, ok := t.(*types.Pointer); ok {
		t = ptr.Elem()
	}
	if named, ok := t.(*types.Named); ok {
		return named.Obj().Name()
	}
	return ""
}
