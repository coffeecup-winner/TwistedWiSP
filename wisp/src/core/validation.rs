use super::{DataType, WispContext, WispFunction};
use std::collections::{HashMap, HashSet};

impl WispContext {
    // Validate reachable source flows without generating IR or executing JIT code.
    pub fn validate_function(&self, name: &str) -> Result<(), String> {
        self.validate_visit(name, &mut HashSet::new(), &mut HashSet::new())
    }

    fn validate_visit(
        &self,
        name: &str,
        visiting: &mut HashSet<String>,
        done: &mut HashSet<String>,
    ) -> Result<(), String> {
        if done.contains(name) {
            return Ok(());
        }

        if !visiting.insert(name.to_owned()) {
            return Err(format!("Recursive function dependency: {name}"));
        }

        let function = self
            .get_function(name)
            .ok_or_else(|| format!("Unknown function: {name}"))?;
        let function = function.borrow();
        if function.outputs().len() > 1 {
            return Err(format!(
                "Multiple outputs are not supported by the compiler: {name}"
            ));
        }

        if name.contains(":stub$") {
            return Err(format!("Invalid math expression in {name}"));
        }

        if let Some(flow) = function.as_flow() {
            let mut indegree: HashMap<_, usize> = flow.node_indices().map(|i| (i, 0)).collect();
            let mut outgoing: HashMap<_, Vec<_>> = HashMap::new();
            let mut array_inputs = HashSet::new();

            for idx in flow.node_indices() {
                let node = flow.get_node(idx).ok_or("Missing node")?;
                self.validate_visit(&node.name, visiting, done)?;
            }

            for edge in flow.edge_indices() {
                let (from, to, connection) =
                    flow.get_connection(edge).ok_or("Missing connection")?;
                let a = self
                    .get_function(&flow.get_node(from).ok_or("Invalid source node")?.name)
                    .ok_or("Unknown source")?;
                let b = self
                    .get_function(&flow.get_node(to).ok_or("Invalid destination node")?.name)
                    .ok_or("Unknown destination")?;
                let a = a.borrow();
                let b = b.borrow();
                let output = a
                    .outputs()
                    .get(connection.output_index as usize)
                    .ok_or_else(|| format!("Invalid output port on {}", a.name()))?;
                let input = b
                    .inputs()
                    .get(connection.input_index as usize)
                    .ok_or_else(|| format!("Invalid input port on {}", b.name()))?;
                if output.type_ != input.type_ {
                    return Err(format!("Incompatible ports: {} → {}", a.name(), b.name()));
                }
                if input.type_ == DataType::Array
                    && !array_inputs.insert((to, connection.input_index))
                {
                    return Err("Array inputs cannot sum multiple connections".into());
                }
                if a.lag_value().is_none() {
                    *indegree.get_mut(&to).ok_or("Invalid destination")? += 1;
                    outgoing.entry(from).or_default().push(to);
                }
            }

            // Detect cycles with Kahn's algorithm
            let mut ready: Vec<_> = indegree
                .iter()
                .filter_map(|(id, n)| (*n == 0).then_some(*id))
                .collect();
            let mut count = 0;
            while let Some(id) = ready.pop() {
                count += 1;
                for to in outgoing.get(&id).into_iter().flatten() {
                    let n = indegree.get_mut(to).unwrap();
                    *n -= 1;
                    if *n == 0 {
                        ready.push(*to);
                    }
                }
            }
            if count != indegree.len() {
                return Err(format!(
                    "Combinational cycle in {name}; feedback needs a lag node"
                ));
            }
        }

        visiting.remove(name);
        done.insert(name.to_owned());

        Ok(())
    }
}
