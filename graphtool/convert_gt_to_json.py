#!/usr/bin/env python3
"""
Convert graph-tool .gt/.gt.gz files to JSON format for bgpsim.

Usage:
    python3 convert_gt_to_json.py input.gt.gz output.json
    python3 convert_gt_to_json.py input.gt.gz  # outputs to stdout
    python3 convert_gt_to_json.py --info input.gt.gz  # just print graph info

This script reads graph-tool graphs and converts them to JSON format
expected by bgpsim's graphtool_loader.

Expected vertex properties:
    - asn: int
    - isd: int
    - border_router: bool
    - core: bool
    - interfaces: vector<int>

Expected edge properties:
    - inter_as: bool
    - rtt_data: double (avg RTT in seconds)
    - distance_km: double
    - skipped_hops: int

Missing properties will use sensible defaults.
"""

import sys
import json
import gzip
from pathlib import Path
from typing import Any

try:
    import graph_tool as gt
    from graph_tool import load_graph
except ImportError:
    print("Error: graph-tool is not installed.", file=sys.stderr)
    print("Install with: conda install -c conda-forge graph-tool", file=sys.stderr)
    print("Or: pip install graph-tool (may not work on all platforms)", file=sys.stderr)
    sys.exit(1)


def print_graph_info(g) -> None:
    """Print information about the graph's properties."""
    print(f"Graph: {g.num_vertices()} vertices, {g.num_edges()} edges", file=sys.stderr)
    print(f"Directed: {g.is_directed()}", file=sys.stderr)

    print("\nVertex properties:", file=sys.stderr)
    for name, prop in g.vertex_properties.items():
        print(f"  - {name}: {prop.value_type()}", file=sys.stderr)

    print("\nEdge properties:", file=sys.stderr)
    for name, prop in g.edge_properties.items():
        print(f"  - {name}: {prop.value_type()}", file=sys.stderr)

    print("\nGraph properties:", file=sys.stderr)
    for name, prop in g.graph_properties.items():
        print(f"  - {name}: {prop}", file=sys.stderr)


def get_property_safe(prop_map, vertex_or_edge, default: Any):
    """Safely get a property value with a default."""
    if prop_map is None:
        return default
    try:
        val = prop_map[vertex_or_edge]
        # Handle numpy types
        if hasattr(val, 'item'):
            return val.item()
        return val
    except (KeyError, IndexError):
        return default


def convert_graph(input_path: str) -> dict:
    """
    Load a graph-tool graph and convert to JSON-serializable dict.

    Args:
        input_path: Path to .gt or .gt.gz file

    Returns:
        Dictionary with 'nodes' and 'edges' keys
    """
    # Load the graph
    g = load_graph(input_path)

    print(f"Loaded graph with {g.num_vertices()} vertices and {g.num_edges()} edges",
          file=sys.stderr)

    # Print available properties for debugging
    print("Available vertex properties:", list(g.vertex_properties.keys()), file=sys.stderr)
    print("Available edge properties:", list(g.edge_properties.keys()), file=sys.stderr)

    # Get vertex property maps (may be None if not present)
    vp_asn = g.vertex_properties.get("asn")
    vp_isd = g.vertex_properties.get("isd")
    vp_border_router = g.vertex_properties.get("border_router")
    vp_core = g.vertex_properties.get("core")
    vp_interfaces = g.vertex_properties.get("interfaces")

    # Get edge property maps
    ep_inter_as = g.edge_properties.get("inter_as")
    ep_rtt_data = g.edge_properties.get("rtt_data")
    ep_distance_km = g.edge_properties.get("distance_km")
    ep_skipped_hops = g.edge_properties.get("skipped_hops")

    # Build AS membership map from edges if asn property missing
    vertex_to_as = {}
    if vp_asn is None:
        # If no ASN property, try to infer from connectivity or use vertex id
        print("Warning: No 'asn' property found, using vertex id as ASN", file=sys.stderr)
        for v in g.vertices():
            vertex_to_as[int(v)] = int(v) + 1
    else:
        for v in g.vertices():
            vertex_to_as[int(v)] = int(get_property_safe(vp_asn, v, int(v) + 1))

    # Convert vertices
    nodes = []
    for v in g.vertices():
        vid = int(v)
        asn = vertex_to_as.get(vid, vid + 1)

        node = {
            "id": vid,
            "asn": asn,
            "isd": int(get_property_safe(vp_isd, v, 1)),
            "border_router": bool(get_property_safe(vp_border_router, v, True)),
            "core": bool(get_property_safe(vp_core, v, False)),
            "interfaces": list(get_property_safe(vp_interfaces, v, [])) if vp_interfaces else [],
        }
        nodes.append(node)

    # Convert edges
    edges = []
    for e in g.edges():
        src_asn = vertex_to_as.get(int(e.source()), 0)
        tgt_asn = vertex_to_as.get(int(e.target()), 0)

        # Determine inter_as: if property exists use it, otherwise infer from ASNs
        if ep_inter_as is not None:
            inter_as = bool(get_property_safe(ep_inter_as, e, False))
        else:
            inter_as = src_asn != tgt_asn

        edge = {
            "source": int(e.source()),
            "target": int(e.target()),
            "inter_as": inter_as,
            "rtt_sec": float(get_property_safe(ep_rtt_data, e, 0.0)),
            "distance_km": float(get_property_safe(ep_distance_km, e, 0.0)),
            "skipped_hops": int(get_property_safe(ep_skipped_hops, e, 0)),
        }
        edges.append(edge)

    return {
        "nodes": nodes,
        "edges": edges,
    }


def main():
    if len(sys.argv) < 2:
        print(f"Usage: {sys.argv[0]} [--info] input.gt.gz [output.json]", file=sys.stderr)
        print(f"  --info: Print graph information without converting", file=sys.stderr)
        sys.exit(1)

    args = sys.argv[1:]

    # Handle --info flag
    if args[0] == "--info":
        if len(args) < 2:
            print("Error: Missing input file", file=sys.stderr)
            sys.exit(1)
        input_path = args[1]
        if not Path(input_path).exists():
            print(f"Error: Input file not found: {input_path}", file=sys.stderr)
            sys.exit(1)
        g = load_graph(input_path)
        print_graph_info(g)
        sys.exit(0)

    input_path = args[0]
    output_path = args[1] if len(args) > 1 else None

    if not Path(input_path).exists():
        print(f"Error: Input file not found: {input_path}", file=sys.stderr)
        sys.exit(1)

    # Convert
    data = convert_graph(input_path)

    # Output
    json_str = json.dumps(data, separators=(',', ':'))  # Compact JSON

    if output_path:
        # Write to file
        if output_path.endswith('.gz'):
            with gzip.open(output_path, 'wt', encoding='utf-8') as f:
                f.write(json_str)
        else:
            with open(output_path, 'w', encoding='utf-8') as f:
                f.write(json_str)
        print(f"Wrote {len(data['nodes'])} nodes and {len(data['edges'])} edges to {output_path}",
              file=sys.stderr)
    else:
        print(json_str)


if __name__ == "__main__":
    main()
