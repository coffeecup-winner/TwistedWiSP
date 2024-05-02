extends "res://flow_graph_node.gd"


func process_watch_updates(values):
	$Value.text = str(values[-1])
