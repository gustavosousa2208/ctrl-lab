% Reference for 03-TF-test.json
%
%   const 1  --a-->[switch]--u--> P(s)=1/(s+1) --y--> [sum + +] --> scope
%   const -1 --b-->   ^                                  ^
%   step     --sel----+                    const 1 ------+
%
% The switch selects input a (=1) while its selector is 0, and input b (=-1)
% once the selector is 1. The selector is a step that goes 0 -> 1 at t = 5, so
% the plant is driven by +1 for t < 5 and -1 afterwards. scope = y + 1.
%
% This is why the references use an explicit per-sample loop: lsim cannot
% express the switch. Run in MATLAB (Control System Toolbox) to regenerate.

Ts = 0.1; end_time = 10;
t = (0:Ts:end_time)';
N = numel(t);

% ZOH-discretized 1/(s+1).
[b, a] = tfdata(c2d(tf(1, [1 1]), Ts, 'zoh'), 'v');
b1 = b(2); a2 = a(2);

sel = double(t >= 5.0);        % step-8 selector, 0 -> 1 at t = 5
ca = 1.0;                      % constant-2 (switch input a)
cb = -1.0;                     % constant-7 (switch input b)
c4 = 1.0;                      % constant-4 (sum offset)

tf1   = zeros(N,1);            % transferFunction-1 output (y)
sw6   = zeros(N,1);            % switch-6 output (u)
sum3  = zeros(N,1);            % sum-3
u_prev = 0.0; y_prev = 0.0;
for k = 1:N
    sw = (sel(k) < 0.5) * ca + (sel(k) >= 0.5) * cb;  % switch: sel 0 -> a, 1 -> b
    y  = b1*u_prev - a2*y_prev;                        % plant from past input
    s  = y + c4;                                        % sum "+ +"

    sw6(k) = sw; tf1(k) = y; sum3(k) = s;
    u_prev = sw; y_prev = y;
end

header = {'t','transferFunction-1','constant-2','sum-3','constant-4', ...
          'scope-5','switch-6','constant-7','step-8'};
data = [t, tf1, ca*ones(N,1), sum3, c4*ones(N,1), sum3, sw6, cb*ones(N,1), sel];

here = fileparts(mfilename('fullpath'));
if isempty(here); here = pwd; end
out = fullfile(here, '03-TF-test.ref.csv');
fid = fopen(out, 'w');
fprintf(fid, '%s\n', strjoin(header, ','));
fmt = [repmat('%.12g,', 1, size(data,2)-1) '%.12g\n'];
fprintf(fid, fmt, data.');
fclose(fid);
fprintf('wrote %s (%d samples)\n', out, size(data,1));
