% Reference for 01-double-integrator.json
%
% Two parallel integrators driven by constants, differenced:
%   const 1.0 -> integrator-2 --a--> [sum + -] --> scope-6
%   const 0.5 -> integrator-4 --b-->
% so scope = int(1.0) - int(0.5) = t - 0.5 t = 0.5 t.
%
% An integrator is the exact ZOH discretization of 1/s: y[k+1] = y[k] + u*Ts,
% output taken before the update (y[0] = initial value). No toolbox needed here.
%
% Run in MATLAB to (re)generate 01-double-integrator.ref.csv for the golden
% harness.

Ts = 0.1; end_time = 10;
t = (0:Ts:end_time)';
N = numel(t);

c1 = 1.0;    % constant-1
c5 = 0.5;    % constant-5

int2 = zeros(N,1); s2 = 0.0;   % integrator-2, initial 0
int4 = zeros(N,1); s4 = 0.0;   % integrator-4, initial 0
for k = 1:N
    int2(k) = s2;  s2 = s2 + c1 * Ts;
    int4(k) = s4;  s4 = s4 + c5 * Ts;
end

sum3  = int2 - int4;           % sum-3, equation "+ -"
scope = sum3;                  % scope-6

header = {'t','constant-1','integrator-2','sum-3','integrator-4', ...
          'constant-5','scope-6'};
data = [t, c1*ones(N,1), int2, sum3, int4, c5*ones(N,1), scope];

here = fileparts(mfilename('fullpath'));
if isempty(here); here = pwd; end
out = fullfile(here, '01-double-integrator.ref.csv');
fid = fopen(out, 'w');
fprintf(fid, '%s\n', strjoin(header, ','));
fmt = [repmat('%.12g,', 1, size(data,2)-1) '%.12g\n'];
fprintf(fid, fmt, data.');
fclose(fid);
fprintf('wrote %s (%d samples)\n', out, size(data,1));
