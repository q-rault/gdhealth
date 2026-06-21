use crate::model::GraphReport;

pub fn render_graph(report: &GraphReport) -> String {
    let mut output = String::from("digraph gdhealth {\n");

    for file in &report.files {
        output.push_str(&format!("  \"{}\";\n", file.path));
    }

    for edge in &report.edges {
        output.push_str(&format!("  \"{}\" -> \"{}\";\n", edge.from, edge.to));
    }

    output.push_str("}\n");
    output
}
