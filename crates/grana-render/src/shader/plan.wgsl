@group(0) @binding(0) var<uniform> schedule: ScheduleData;
@group(0) @binding(1) var<storage, read_write> plan: array<atomic<u32>>;

@compute @workgroup_size(1)
fn plan_main() {
    let capacity = schedule.capacity;
    var demand = 0u;
    var full = false;
    for (var bucket = 0u; bucket < BUCKETS; bucket += 1u) {
        let visible = atomicLoad(&plan[VISIBLE_BASE + bucket]);
        atomicStore(&plan[FILL_BASE + bucket], 0u);
        if (visible == 0u) {
            atomicStore(&plan[BASE_BASE + bucket], demand);
            continue;
        }
        if (full || demand + visible > capacity) {
            full = true;
            atomicStore(&plan[BASE_BASE + bucket], EXCLUDED);
            continue;
        }
        atomicStore(&plan[BASE_BASE + bucket], demand);
        demand = demand + visible;
    }
    atomicStore(&plan[1], demand);
}
